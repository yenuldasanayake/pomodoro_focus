use crate::{
    alt_tab, audio, site_blocker,
    storage::{History, Notes, Settings, Storage, StudyNote},
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

const HISTORY_FLUSH_INTERVAL: Duration = Duration::from_secs(10);

pub struct AppState {
    pub settings: Settings,
    pub history: History,
    pub notes: Notes,
    pub timer: Timer,
    pub pin: String,
    pub local_url: String,
    pub sites_blocked: bool,
    pub hosts_block_active: bool,
    pub alt_tab_block_active: bool,
    pub alt_tab_error: Option<String>,
    pub server_error: Option<String>,
    pub pending_note_duration: Option<u64>,
    pub error_message: Option<String>,
    pub ctx: egui::Context,
    auth_failures: u32,
    auth_window_started: Instant,
    auth_locked_until: Option<Instant>,
    pub storage: Storage,
    history_dirty: bool,
    last_history_flush: Instant,
}

impl AppState {
    pub fn new(ctx: egui::Context, storage: Storage, pin: String, local_url: String) -> Self {
        let mut warnings: Vec<String> = Vec::new();
        let mut settings = load_with_warning(storage.load_settings(), "settings", &mut warnings);
        let history = load_with_warning(storage.load_history(), "history", &mut warnings);
        let notes = load_with_warning(storage.load_notes(), "notes", &mut warnings);

        // One-shot migration from the old data/blocking.json file. If it exists
        // and contains sites_blocked: true, carry the value into settings.json
        // and remove the legacy file. Safe to run repeatedly; does nothing once
        // the file is gone.
        if let Some(legacy) = read_legacy_blocking() {
            if legacy && !settings.sites_blocked {
                settings.sites_blocked = legacy;
                if let Err(error) = storage.save_settings(&settings) {
                    eprintln!("Could not migrate legacy blocking.json: {error}");
                }
            }
            let path = std::path::Path::new("data").join("blocking.json");
            if let Err(error) = std::fs::remove_file(&path) {
                eprintln!("Could not remove legacy {}: {error}", path.display());
            }
        }

        let timer = Timer::new(&settings);

        let (alt_tab_block_active, alt_tab_error) = if settings.block_alt_tab {
            match alt_tab::set_blocked(true) {
                Ok(()) => (true, None),
                Err(error) => (false, Some(format!("Could not block Alt+Tab: {error}"))),
            }
        } else {
            (false, None)
        };

        let hosts_block_active = match site_blocker::set_blocked(settings.sites_blocked) {
            Ok(()) => settings.sites_blocked,
            Err(error) => {
                warnings.push(format!(
                    "Could not synchronize the Windows hosts block: {error}. \
                     Run the app as Administrator."
                ));
                false
            }
        };

        let error_message = if warnings.is_empty() {
            None
        } else {
            Some(warnings.join(" · "))
        };

        Self {
            sites_blocked: settings.sites_blocked,
            hosts_block_active,
            settings,
            history,
            notes,
            timer,
            pin,
            local_url,
            alt_tab_block_active,
            alt_tab_error,
            server_error: None,
            pending_note_duration: None,
            error_message,
            ctx,
            auth_failures: 0,
            auth_window_started: Instant::now(),
            auth_locked_until: None,
            storage,
            history_dirty: false,
            last_history_flush: Instant::now(),
        }
    }

    pub fn tick(&mut self) {
        let completed = self.timer.update(&self.settings);

        let studied = self.timer.take_study_seconds();
        if studied > 0 {
            self.history.add_today(studied);
            self.history_dirty = true;
        }

        if self.history_dirty && self.last_history_flush.elapsed() >= HISTORY_FLUSH_INTERVAL {
            self.flush_history();
        }

        if let Some(mode) = completed {
            audio::play_completion_beep();
            if mode == Mode::Study {
                self.pending_note_duration = Some(u64::from(self.settings.study_minutes) * 60);
            }
            self.flush_history();
            self.ctx.request_repaint();
        }
    }

    pub fn flush_history(&mut self) {
        if !self.history_dirty {
            return;
        }
        match self.storage.save_history(&self.history) {
            Ok(()) => {
                self.history_dirty = false;
                self.last_history_flush = Instant::now();
            }
            Err(error) => {
                self.error_message = Some(format!("Could not save study history: {error}"));
            }
        }
    }

    pub fn flush_all(&mut self) {
        self.flush_history();
        if let Err(error) = self.storage.save_notes(&self.notes) {
            eprintln!("Could not save notes on exit: {error}");
        }
        if let Err(error) = self.storage.save_settings(&self.settings) {
            eprintln!("Could not save settings on exit: {error}");
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
        settings.block_alt_tab = self.settings.block_alt_tab;
        settings.sites_blocked = self.settings.sites_blocked;
        self.storage
            .save_settings(&settings)
            .map_err(|error| format!("Could not save settings: {error}"))?;
        self.settings = settings;
        self.timer.set_settings(&self.settings);
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn set_alt_tab_blocked(&mut self, blocked: bool) -> Result<(), String> {
        if self.alt_tab_block_active == blocked && self.settings.block_alt_tab == blocked {
            return Ok(());
        }
        let previous_active = self.alt_tab_block_active;
        if let Err(error) = alt_tab::set_blocked(blocked) {
            let message = format!("Could not update Alt+Tab blocking: {error}");
            self.alt_tab_error = Some(message.clone());
            self.ctx.request_repaint();
            return Err(message);
        }

        let mut settings = self.settings.clone();
        settings.block_alt_tab = blocked;
        if let Err(error) = self.storage.save_settings(&settings) {
            if previous_active != blocked
                && let Err(rollback_error) = alt_tab::set_blocked(previous_active)
            {
                let message = format!(
                    "Could not save Alt+Tab setting: {error}; restoring the previous \
                     keyboard state also failed: {rollback_error}"
                );
                self.alt_tab_error = Some(message.clone());
                self.ctx.request_repaint();
                return Err(message);
            }
            let message = format!("Could not save Alt+Tab setting: {error}");
            self.alt_tab_error = Some(message.clone());
            self.ctx.request_repaint();
            return Err(message);
        }

        self.settings = settings;
        self.alt_tab_block_active = blocked;
        self.alt_tab_error = None;
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
                    "Browser extension blocking is enabled, but system-wide blocking \
                     failed: {error}. Run Pomodoro Focus as Administrator."
                ));
                false
            }
            Err(error) => {
                return Err(format!(
                    "Could not disable system-wide blocking: {error}. \
                     Run Pomodoro Focus as Administrator."
                ));
            }
        };

        let mut settings = self.settings.clone();
        settings.sites_blocked = blocked;
        if let Err(error) = self.storage.save_settings(&settings) {
            if hosts_block_active != old_hosts_state
                && let Err(rollback_error) = site_blocker::set_blocked(old_hosts_state)
            {
                return Err(format!(
                    "Could not save blocking settings: {error}; \
                     reverting the hosts file also failed: {rollback_error}"
                ));
            }
            return Err(format!("Could not save blocking settings: {error}"));
        }

        self.settings = settings;
        self.sites_blocked = blocked;
        self.hosts_block_active = hosts_block_active;

        if blocked && !hosts_block_active {
            self.error_message.get_or_insert_with(|| {
                "Browser extension blocking is enabled, but system-wide blocking \
                 needs Administrator access."
                    .to_owned()
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
            self.history_dirty = true;
            self.flush_history();
        }
    }
}

fn load_with_warning<T>(
    result: std::io::Result<T>,
    name: &str,
    warnings: &mut Vec<String>,
) -> T
where
    T: Default,
{
    result.unwrap_or_else(|error| {
        warnings.push(format!("Could not load {name}: {error}"));
        T::default()
    })
}

fn read_legacy_blocking() -> Option<bool> {
    #[derive(Deserialize)]
    struct Legacy {
        #[serde(default)]
        sites_blocked: bool,
    }
    let path = std::path::Path::new("data").join("blocking.json");
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice::<Legacy>(&bytes)
        .ok()
        .map(|legacy| legacy.sites_blocked)
}

pub fn generate_pin() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes)?;
    // Avoid leading zeros so phone entry is unambiguous.
    let value = 10_000_000 + (u32::from_ne_bytes(bytes) % 90_000_000);
    Ok(value.to_string())
}

pub fn local_url() -> String {
    local_url_for_port(5000)
}

pub fn local_url_for_port(port: u16) -> String {
    match local_ip_address::local_ip() {
        Ok(ip) => format!("http://{ip}:{port}"),
        Err(error) => {
            eprintln!("Could not determine the PC's local IP address: {error}");
            format!("http://127.0.0.1:{port}")
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

        let listener = match runtime.block_on(TcpListener::bind("0.0.0.0:0")) {
            Ok(listener) => listener,
            Err(error) => {
                report_server_error(
                    &state,
                    format!("Could not bind phone server: {error}"),
                );
                return;
            }
        };

        let port = match listener.local_addr() {
            Ok(address) => address.port(),
            Err(error) => {
                report_server_error(
                    &state,
                    format!("Could not read phone server port: {error}"),
                );
                return;
            }
        };

        if let Ok(mut state) = state.lock() {
            state.local_url = local_url_for_port(port);
            state.ctx.request_repaint();
        }

        let router = Router::new()
            .route("/", get(mobile_page))
            .route("/manifest.json", get(manifest))
            .route("/icon.svg", get(icon))
            .route("/status", get(status))
            .route("/command", post(command))
            .route("/settings", post(update_settings))
            .route("/blocking", post(update_blocking))
            .route("/alt-tab", post(update_alt_tab))
            .route("/notes", post(add_note))
            .with_state(state.clone());

        eprintln!("Phone remote listening on 0.0.0.0:{port}");
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
    Html(include_str!("../web/index.html"))
}

async fn manifest() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/manifest+json")],
        include_str!("../web/manifest.json"),
    )
}

async fn icon() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
        include_str!("../web/icon.svg"),
    )
}

#[derive(Serialize)]
struct NoteSummary {
    timestamp: String,
    duration_seconds: u64,
    accomplishment: String,
    subject: Option<String>,
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
    alt_tab_blocked: bool,
    alt_tab_error: Option<String>,
    current_streak: u32,
    completed_sessions: u32,
    notes: Vec<NoteSummary>,
}

async fn status(State(shared): State<SharedState>) -> Result<Json<StatusResponse>, ApiError> {
    let mut state = shared
        .lock()
        .map_err(|_| ApiError::internal("App state is unavailable"))?;
    state.tick();

    let notes: Vec<NoteSummary> = state
        .notes
        .entries
        .iter()
        .rev()
        .take(20)
        .map(|note| NoteSummary {
            timestamp: note.timestamp.clone(),
            duration_seconds: note.duration_seconds,
            accomplishment: note.accomplishment.clone(),
            subject: note.subject.clone(),
        })
        .collect();

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
        alt_tab_blocked: state.alt_tab_block_active,
        alt_tab_error: state.alt_tab_error.clone(),
        current_streak: state.history.current_streak(),
        completed_sessions: state.timer.completed_study_sessions,
        notes,
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
        block_alt_tab: state.settings.block_alt_tab,
        sites_blocked: state.settings.sites_blocked,
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
struct AltTabRequest {
    token: String,
    blocked: bool,
}

async fn update_alt_tab(
    State(shared): State<SharedState>,
    Json(request): Json<AltTabRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared
        .lock()
        .map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, &request.token)?;
    state
        .set_alt_tab_blocked(request.blocked)
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

#[derive(Deserialize)]
struct NoteRequest {
    token: String,
    accomplishment: String,
    #[serde(default)]
    subject: String,
}

async fn add_note(
    State(shared): State<SharedState>,
    Json(request): Json<NoteRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared
        .lock()
        .map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, &request.token)?;
    let subject = if request.subject.trim().is_empty() {
        None
    } else {
        Some(request.subject)
    };
    state
        .save_note(request.accomplishment, subject)
        .map_err(ApiError::bad_request)?;
    Ok(Json(MessageResponse {
        ok: true,
        warning: None,
    }))
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