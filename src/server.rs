use crate::{
    audio, site_blocker,
    storage::{BlockingSettings, History, Notes, Settings, Storage, StudyNote},
    timer::{Mode, Timer},
};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chrono::Local;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tokio::net::TcpListener;

pub type SharedState = Arc<Mutex<AppState>>;

pub struct AppState {
    pub settings: Settings,
    pub history: History,
    pub notes: Notes,
    pub timer: Timer,
    pub pin: String,
    pub local_url: String,
    pub sites_blocked: bool,
    pub hosts_block_active: bool,
    pub server_error: Option<String>,
    pub pending_note_duration: Option<u64>,
    pub error_message: Option<String>,
    pub ctx: egui::Context,
    auth_failures: u32,
    auth_window_started: Instant,
    auth_locked_until: Option<Instant>,
    storage: Storage,
}

impl AppState {
    pub fn new(ctx: egui::Context, storage: Storage, pin: String, local_url: String) -> Self {
        let settings = load_with_warning(storage.load_settings(), "settings");
        let history = load_with_warning(storage.load_history(), "history");
        let notes = load_with_warning(storage.load_notes(), "notes");
        let blocking = load_with_warning(storage.load_blocking_settings(), "blocking");
        let timer = Timer::new(&settings);
        let mut block_error = None;
        let hosts_block_active = match site_blocker::set_blocked(blocking.sites_blocked) {
            Ok(()) => blocking.sites_blocked,
            Err(error) => {
                block_error = Some(format!(
                    "Could not synchronize the Windows hosts block: {error}. Run the app as Administrator."
                ));
                false
            }
        };
        Self {
            settings,
            history,
            notes,
            timer,
            pin,
            local_url,
            sites_blocked: blocking.sites_blocked,
            hosts_block_active,
            server_error: None,
            pending_note_duration: None,
            error_message: block_error,
            ctx,
            auth_failures: 0,
            auth_window_started: Instant::now(),
            auth_locked_until: None,
            storage,
        }
    }

    pub fn tick(&mut self) {
        let completed = self.timer.update(&self.settings);
        let studied = self.timer.take_study_seconds();
        if studied > 0 {
            self.history.add_today(studied);
            if let Err(error) = self.storage.save_history(&self.history) {
                self.error_message = Some(format!("Could not save study history: {error}"));
            }
        }
        if let Some(mode) = completed {
            audio::play_completion_beep();
            if mode == Mode::Study {
                self.pending_note_duration = Some(u64::from(self.settings.study_minutes) * 60);
            }
            self.ctx.request_repaint();
        }
    }

    pub fn execute_command(&mut self, command: &str) -> Result<(), String> {
        self.tick();
        match command {
            "start" => self.timer.start(),
            "pause" => self.timer.pause(),
            "reset" => self.timer.reset(&self.settings),
            "skip" => self.timer.skip(&self.settings),
            _ => return Err("Command must be start, pause, reset, or skip".to_owned()),
        }
        self.collect_study_time();
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn update_settings(&mut self, mut settings: Settings) -> Result<(), String> {
        self.tick();
        settings.sanitize();
        self.storage
            .save_settings(&settings)
            .map_err(|error| format!("Could not save settings: {error}"))?;
        self.settings = settings;
        self.timer.set_settings(&self.settings);
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn save_note(
        &mut self,
        accomplishment: String,
        subject: Option<String>,
    ) -> Result<(), String> {
        let accomplishment = accomplishment.trim().to_owned();
        let subject = subject
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let duration = self.pending_note_duration.unwrap_or_default();
        self.notes.entries.push(StudyNote {
            timestamp: Local::now().to_rfc3339(),
            duration_seconds: duration,
            accomplishment,
            subject,
        });
        if let Err(error) = self.storage.save_notes(&self.notes) {
            self.notes.entries.pop();
            return Err(format!("Could not save study note: {error}"));
        }
        self.pending_note_duration = None;
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn set_sites_blocked(&mut self, blocked: bool) -> Result<(), String> {
        if self.sites_blocked == blocked {
            return Ok(());
        }
        let old_hosts_state = self.hosts_block_active;
        let hosts_block_active = match site_blocker::set_blocked(blocked) {
            Ok(()) => blocked,
            Err(error) if blocked => {
                self.error_message = Some(format!(
                    "Browser extension blocking is enabled, but system-wide blocking failed: {error}. Run Pomodoro Focus as Administrator."
                ));
                false
            }
            Err(error) => {
                return Err(format!(
                    "Could not disable system-wide blocking: {error}. Run Pomodoro Focus as Administrator."
                ));
            }
        };

        let settings = BlockingSettings {
            sites_blocked: blocked,
        };
        if let Err(error) = self.storage.save_blocking_settings(&settings) {
            if hosts_block_active != old_hosts_state
                && let Err(rollback_error) = site_blocker::set_blocked(old_hosts_state)
            {
                return Err(format!(
                    "Could not save blocking settings: {error}; reverting the hosts file also failed: {rollback_error}"
                ));
            }
            return Err(format!("Could not save blocking settings: {error}"));
        }
        self.sites_blocked = blocked;
        self.hosts_block_active = hosts_block_active;
        if blocked && !hosts_block_active {
            self.error_message.get_or_insert_with(|| {
                "Browser extension blocking is enabled, but system-wide blocking needs Administrator access.".to_owned()
            });
        } else {
            self.error_message = None;
        }
        self.ctx.request_repaint();
        Ok(())
    }

    fn collect_study_time(&mut self) {
        let studied = self.timer.take_study_seconds();
        if studied > 0 {
            self.history.add_today(studied);
            if let Err(error) = self.storage.save_history(&self.history) {
                self.error_message = Some(format!("Could not save study history: {error}"));
            }
        }
    }
}

fn load_with_warning<T>(result: std::io::Result<T>, name: &str) -> T
where
    T: Default,
{
    result.unwrap_or_else(|error| {
        eprintln!("Could not load {name}: {error}; using defaults");
        T::default()
    })
}

pub fn generate_pin() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 4];
    getrandom::fill(&mut bytes)?;
    Ok(format!("{:08}", u32::from_ne_bytes(bytes) % 100_000_000))
}

pub fn local_url() -> String {
    match local_ip_address::local_ip() {
        Ok(ip) => format!("http://{ip}:5000"),
        Err(error) => {
            eprintln!("Could not determine the PC's local IP address: {error}");
            "http://127.0.0.1:5000".to_owned()
        }
    }
}

pub fn start_server(state: SharedState) {
    thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                report_server_error(&state, format!("Could not start phone server: {error}"));
                return;
            }
        };
        let listener = match runtime.block_on(TcpListener::bind("0.0.0.0:5000")) {
            Ok(listener) => listener,
            Err(error) => {
                report_server_error(
                    &state,
                    format!("Could not bind phone server on port 5000: {error}"),
                );
                return;
            }
        };
        let router = Router::new()
            .route("/", get(mobile_page))
            .route("/status", get(status))
            .route("/command", post(command))
            .route("/settings", post(update_settings))
            .route("/blocking", post(update_blocking))
            .with_state(state.clone());
        match listener.local_addr() {
            Ok(address) => eprintln!("Phone remote listening on {address}"),
            Err(error) => {
                eprintln!("Phone remote is listening, but its address could not be read: {error}")
            }
        }
        let result = runtime.block_on(async { axum::serve(listener, router).await });
        if let Err(error) = result {
            report_server_error(&state, format!("Phone server stopped: {error}"));
        }
    });
}

fn report_server_error(state: &SharedState, message: String) {
    eprintln!("{message}");
    if let Ok(mut state) = state.lock() {
        state.server_error = Some(message);
        state.ctx.request_repaint();
    }
}

async fn mobile_page() -> Html<&'static str> {
    Html(MOBILE_PAGE)
}

#[derive(Serialize)]
struct StatusResponse {
    remaining_seconds: u64,
    mode: Mode,
    today_total_seconds: u64,
    running: bool,
    study_minutes: u32,
    short_break_minutes: u32,
    long_break_minutes: u32,
    sessions_before_long_break: u32,
    escape_exits_fullscreen: bool,
    sites_blocked: bool,
    hosts_block_active: bool,
}

async fn status(State(shared): State<SharedState>) -> Result<Json<StatusResponse>, ApiError> {
    let mut state = shared
        .lock()
        .map_err(|_| ApiError::internal("App state is unavailable"))?;
    state.tick();
    Ok(Json(StatusResponse {
        remaining_seconds: state.timer.remaining_seconds,
        mode: state.timer.mode,
        today_total_seconds: state.history.today_seconds(),
        running: state.timer.running,
        study_minutes: state.settings.study_minutes,
        short_break_minutes: state.settings.short_break_minutes,
        long_break_minutes: state.settings.long_break_minutes,
        sessions_before_long_break: state.settings.sessions_before_long_break,
        escape_exits_fullscreen: state.settings.escape_exits_fullscreen,
        sites_blocked: state.sites_blocked,
        hosts_block_active: state.hosts_block_active,
    }))
}

#[derive(Deserialize)]
struct CommandRequest {
    token: String,
    command: String,
}

async fn command(
    State(shared): State<SharedState>,
    Json(request): Json<CommandRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared
        .lock()
        .map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, &request.token)?;
    state
        .execute_command(&request.command)
        .map_err(ApiError::bad_request)?;
    Ok(Json(MessageResponse {
        ok: true,
        warning: None,
    }))
}

#[derive(Deserialize)]
struct SettingsRequest {
    token: String,
    study_minutes: u32,
    short_break_minutes: u32,
    long_break_minutes: u32,
    sessions_before_long_break: u32,
    escape_exits_fullscreen: bool,
}

async fn update_settings(
    State(shared): State<SharedState>,
    Json(request): Json<SettingsRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared
        .lock()
        .map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, &request.token)?;
    let settings = Settings {
        study_minutes: request.study_minutes,
        short_break_minutes: request.short_break_minutes,
        long_break_minutes: request.long_break_minutes,
        sessions_before_long_break: request.sessions_before_long_break,
        escape_exits_fullscreen: request.escape_exits_fullscreen,
    };
    state
        .update_settings(settings)
        .map_err(ApiError::internal)?;
    Ok(Json(MessageResponse {
        ok: true,
        warning: None,
    }))
}

#[derive(Deserialize)]
struct BlockingRequest {
    token: String,
    blocked: bool,
}

async fn update_blocking(
    State(shared): State<SharedState>,
    Json(request): Json<BlockingRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared
        .lock()
        .map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, &request.token)?;
    state
        .set_sites_blocked(request.blocked)
        .map_err(ApiError::internal)?;
    let warning = if request.blocked && !state.hosts_block_active {
        state.error_message.clone()
    } else {
        None
    };
    Ok(Json(MessageResponse { ok: true, warning }))
}

fn verify_token(state: &mut AppState, token: &str) -> Result<(), ApiError> {
    let now = Instant::now();
    if state.auth_locked_until.is_some_and(|until| now < until) {
        return Err(ApiError::rate_limited(
            "Too many incorrect PIN attempts. Try again in one minute.",
        ));
    }
    if now.duration_since(state.auth_window_started) >= Duration::from_secs(60) {
        state.auth_window_started = now;
        state.auth_failures = 0;
        state.auth_locked_until = None;
    }
    if state.pin == token {
        state.auth_failures = 0;
        state.auth_window_started = now;
        Ok(())
    } else {
        state.auth_failures += 1;
        if state.auth_failures >= 5 {
            state.auth_locked_until = Some(now + Duration::from_secs(60));
        }
        Err(ApiError::unauthorized("Incorrect PIN"))
    }
}

#[derive(Serialize)]
struct MessageResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

struct ApiError(StatusCode, String);

impl ApiError {
    fn bad_request(message: String) -> Self {
        Self(StatusCode::BAD_REQUEST, message)
    }

    fn unauthorized(message: &str) -> Self {
        Self(StatusCode::UNAUTHORIZED, message.to_owned())
    }

    fn internal(message: impl Into<String>) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, message.into())
    }

    fn rate_limited(message: &str) -> Self {
        Self(StatusCode::TOO_MANY_REQUESTS, message.to_owned())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

const MOBILE_PAGE: &str = r###"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="theme-color" content="#181825"><title>Pomodoro Focus</title>
<style>
:root{color-scheme:dark;--bg:#181825;--panel:#1e1e2e;--text:#e6e6ef;--muted:#9394a6;--accent:#f38ba8}
*{box-sizing:border-box}body{margin:0;background:linear-gradient(155deg,#181825,#11111b);color:var(--text);font:16px system-ui,sans-serif;min-height:100vh}
main{width:min(100% - 32px,460px);margin:0 auto;padding:34px 0 48px}.eyebrow{color:var(--muted);letter-spacing:.18em;font-size:.72rem;font-weight:700}
h1{font-size:1.3rem;margin:5px 0 24px}.card{background:#1e1e2ee8;border:1px solid #313244;border-radius:22px;padding:26px;margin:16px 0}
.timer{text-align:center}.mode{color:var(--accent);font-size:.76rem;letter-spacing:.2em;font-weight:700}.clock{font-size:clamp(4.8rem,24vw,7rem);font-weight:200;letter-spacing:-.08em;font-variant-numeric:tabular-nums;line-height:1.15}
.today{color:var(--muted);font-size:.9rem}.buttons{display:flex;gap:10px;margin-top:22px}.buttons button{flex:1}.button{border:0;border-radius:14px;padding:13px 16px;background:#313244;color:var(--text);font-weight:650;font-size:.95rem}.button.primary{background:var(--accent);color:#181825}.button:active{transform:scale(.98)}
label{display:block;color:var(--muted);font-size:.82rem;margin:13px 0 6px}input:not([type=checkbox]){width:100%;padding:12px;border:1px solid #45475a;border-radius:11px;background:#11111b;color:var(--text);font:inherit}.check-row{display:flex;align-items:center;gap:10px}.check-row input{width:18px;height:18px;accent-color:var(--accent)}
.grid{display:grid;grid-template-columns:1fr 1fr;gap:4px 12px}.token{display:flex;gap:8px}.token input{flex:1}button{cursor:pointer}.message{min-height:20px;color:#a6e3a1;font-size:.84rem;margin-top:8px}
</style></head>
<body><main>
<div class="eyebrow">POMODORO FOCUS</div><h1>Your focus, in sync.</h1>
<section class="card timer"><div id="mode" class="mode">CONNECTING</div><div id="clock" class="clock">--:--</div><div id="today" class="today">Today · 0m</div>
<div class="buttons"><button class="button primary" id="toggle">Start</button><button class="button" onclick="sendCommand('reset')">Reset</button><button class="button" onclick="sendCommand('skip')">Skip</button></div></section>
<section class="card"><div class="eyebrow">STAY FOCUSED</div><p id="block-description" class="today">Block Instagram, YouTube, TikTok, and Facebook.</p><button id="block-toggle" class="button primary" style="width:100%" onclick="toggleBlocking()">Block distracting sites</button></section>
<section class="card"><div class="eyebrow">REMOTE ACCESS</div><label for="pin">PIN shown on your PC</label><div class="token"><input id="pin" inputmode="numeric" maxlength="8" placeholder="8-digit PIN"></div>
<div class="eyebrow" style="margin-top:24px">TIMER SETTINGS</div><div class="grid">
<div><label>Focus · minutes</label><input id="study" type="number" min="1" max="180"></div><div><label>Short break · minutes</label><input id="short" type="number" min="1" max="60"></div>
<div><label>Long break · minutes</label><input id="long" type="number" min="1" max="120"></div><div><label>Focus sessions</label><input id="sessions" type="number" min="1" max="12"></div></div>
<label class="check-row"><input id="escape-enabled" type="checkbox"> Allow Esc to toggle fullscreen</label>
<button class="button primary" style="width:100%;margin-top:16px" onclick="saveSettings()">Save settings</button><div id="message" class="message"></div></section>
</main><script>
const colors={study:'#f38ba8',short_break:'#a6e3a1',long_break:'#89b4fa'};
const field=id=>document.getElementById(id);
function pin(){return document.querySelector('#pin').value.trim()}
async function post(path,data){const response=await fetch(path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({...data,token:pin()})});const result=await response.json();if(!response.ok)throw Error(result.error||'Request failed');return result}
async function sendCommand(command){try{await post('/command',{command});message('Command sent')}catch(error){message(error.message,true)}}
async function toggleBlocking(){try{const blocked=document.querySelector('#block-toggle').dataset.blocked!=='true';const result=await post('/blocking',{blocked});message(result.warning|| (blocked?'Distracting sites blocked':'Sites unblocked'),Boolean(result.warning))}catch(error){message(error.message,true)}}
function message(text,error=false){const el=document.querySelector('#message');el.textContent=text;el.style.color=error?'#f38ba8':'#a6e3a1'}
async function saveSettings(){try{await post('/settings',{study_minutes:+field('study').value,short_break_minutes:+field('short').value,long_break_minutes:+field('long').value,sessions_before_long_break:+field('sessions').value,escape_exits_fullscreen:field('escape-enabled').checked});message('Settings saved')}catch(error){message(error.message,true)}}
let loaded=false;
async function refresh(){try{const s=await(await fetch('/status')).json();const mode=s.mode;document.documentElement.style.setProperty('--accent',colors[mode]||'#f38ba8');document.querySelector('#mode').textContent=mode.replace('_',' ').toUpperCase();document.querySelector('#clock').textContent=String(Math.floor(s.remaining_seconds/60)).padStart(2,'0')+':'+String(s.remaining_seconds%60).padStart(2,'0');document.querySelector('#today').textContent='Today · '+Math.floor(s.today_total_seconds/3600)+'h '+Math.floor(s.today_total_seconds%3600/60)+'m';document.querySelector('#toggle').textContent=s.running?'Pause':'Start';document.querySelector('#toggle').onclick=()=>sendCommand(s.running?'pause':'start');const blockButton=document.querySelector('#block-toggle');blockButton.dataset.blocked=String(s.sites_blocked);blockButton.textContent=s.sites_blocked?'Unblock distracting sites':'Block distracting sites';document.querySelector('#block-description').textContent=s.hosts_block_active?'Blocking is active across Windows and in the browser extension.':s.sites_blocked?'Browser extension blocking is on. Run the desktop app as Administrator for system-wide blocking.':'Block Instagram, YouTube, TikTok, and Facebook across Windows and supported browsers.';if(!loaded){field('study').value=s.study_minutes;field('short').value=s.short_break_minutes;field('long').value=s.long_break_minutes;field('sessions').value=s.sessions_before_long_break;field('escape-enabled').checked=s.escape_exits_fullscreen;loaded=true}}catch(_){document.querySelector('#mode').textContent='PC NOT CONNECTED'}}
refresh();setInterval(refresh,1000);
</script></body></html>"###;
