use crate::{
    audio, breaks, export,
    storage::{
        History, Notes, Settings, Storage, StudyNote, WeeklyReflection, WeeklyReflections,
        generate_id,
    },
    system,
    timer::{Mode, Timer},
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chrono::Local;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime},
};
use tokio::net::TcpListener;

pub type SharedState = Arc<Mutex<AppState>>;

const HISTORY_FLUSH_INTERVAL: Duration = Duration::from_secs(10);
const AUTH_WINDOW: Duration = Duration::from_secs(60);
const AUTH_LOCKOUT: Duration = Duration::from_secs(60);
const AUTH_MAX_FAILURES: u32 = 5;
const AUTH_RECORD_TTL: Duration = Duration::from_secs(300);
const UNDO_WINDOW: Duration = Duration::from_secs(30);
const POWER_ACTION_DELAY_SECS: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerAction {
    Shutdown,
    Restart,
}

#[derive(Debug, Clone)]
pub struct PendingPowerAction {
    pub action: PowerAction,
    pub deadline: Instant,
}

pub struct AppState {
    pub settings: Settings,
    pub history: History,
    pub notes: Notes,
    pub reflections: WeeklyReflections,
    pub current_week_reflection: WeeklyReflection,
    pub timer: Timer,
    pub pin: String,
    pub port: u16,
    pub port_change_pending: bool,
    pub local_url: String,
    pub server_error: Option<String>,
    pub pending_note_duration: Option<u64>,
    pub task_prompt_pending: bool,
    pub pending_power_action: Option<PendingPowerAction>,
    pub should_restore_window: bool,
    pub error_message: Option<String>,
    pub ctx: egui::Context,
    pub storage: Storage,
    auth: AuthState,
    undo_deleted_notes: Vec<(Instant, StudyNote)>,
    history_dirty: bool,
    last_history_flush: Instant,
    timer_dirty: bool,
    last_timer_flush: Instant,
    last_wall_check: SystemTime,
    last_monotonic_check: Instant,
}

#[derive(Default)]
struct AuthState {
    failures: HashMap<IpAddr, FailureRecord>,
}

struct FailureRecord {
    count: u32,
    window_started: Instant,
    locked_until: Option<Instant>,
    last_seen: Instant,
}

impl AuthState {
    fn prune(&mut self, now: Instant) {
        self.failures.retain(|_, record| now.duration_since(record.last_seen) < AUTH_RECORD_TTL);
    }

    fn record_success(&mut self, ip: IpAddr) {
        self.failures.remove(&ip);
    }

    fn record_failure(&mut self, ip: IpAddr, now: Instant) -> bool {
        self.prune(now);
        let record = self.failures.entry(ip).or_insert_with(|| FailureRecord {
            count: 0,
            window_started: now,
            locked_until: None,
            last_seen: now,
        });
        if now.duration_since(record.window_started) >= AUTH_WINDOW {
            record.count = 0;
            record.window_started = now;
            record.locked_until = None;
        }
        record.count += 1;
        record.last_seen = now;
        if record.count >= AUTH_MAX_FAILURES {
            record.locked_until = Some(now + AUTH_LOCKOUT);
            true
        } else {
            false
        }
    }
}

impl AppState {
    pub fn new(ctx: egui::Context, storage: Storage) -> Result<Self, getrandom::Error> {
        let mut warnings: Vec<String> = Vec::new();

        if let Err(error) = storage.check_writable() {
            warnings.push(error.to_string());
        }

        let mut settings = load_with_warning(storage.load_settings(), "settings", &mut warnings);
        settings.launch_at_startup = system::startup_enabled();
        let history = load_with_warning(storage.load_history(), "history", &mut warnings);
        let notes = load_with_warning(storage.load_notes(), "notes", &mut warnings);
        let reflections =
            load_with_warning(storage.load_reflections(), "weekly reflections", &mut warnings);
        let current_week_reflection = reflections.for_week(&WeeklyReflection::current_week_start());

        if settings.pin.is_empty() {
            settings.pin = generate_pin()?;
            if let Err(error) = storage.save_settings(&settings) {
                warnings.push(format!("Could not save the initial PIN: {error}"));
            }
        }

        let timer = match storage.load_timer() {
            Ok(snapshot) => Timer::restore(&settings, snapshot),
            Err(error) => {
                warnings.push(format!("Could not load saved timer state: {error}"));
                Timer::new(&settings)
            }
        };

        let error_message = if warnings.is_empty() { None } else { Some(warnings.join(" · ")) };

        let pin = settings.pin.clone();
        let port = settings.port;

        Ok(Self {
            settings,
            history,
            notes,
            reflections,
            current_week_reflection,
            timer,
            pin,
            port,
            port_change_pending: false,
            local_url: String::new(),
            server_error: None,
            pending_note_duration: None,
            task_prompt_pending: false,
            pending_power_action: None,
            should_restore_window: false,
            error_message,
            ctx,
            storage,
            auth: AuthState::default(),
            undo_deleted_notes: Vec::new(),
            history_dirty: false,
            last_history_flush: Instant::now(),
            timer_dirty: false,
            last_timer_flush: Instant::now(),
            last_wall_check: SystemTime::now(),
            last_monotonic_check: Instant::now(),
        })
    }

    pub fn undo_available(&self) -> bool {
        self.undo_deleted_notes.last().is_some_and(|(when, _)| when.elapsed() < UNDO_WINDOW)
    }

    pub fn power_control_available(&self) -> bool {
        self.settings.remote_power_control
    }

    pub fn import_available(&self) -> bool {
        self.settings.remote_import
    }

    pub fn pending_power_seconds(&self) -> Option<u32> {
        self.pending_power_action
            .as_ref()
            .map(|p| p.deadline.saturating_duration_since(Instant::now()).as_secs() as u32)
            .filter(|&s| s > 0)
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        let week_start = WeeklyReflection::current_week_start();
        if self.current_week_reflection.week_start != week_start {
            self.current_week_reflection = self.reflections.for_week(&week_start);
        }

        let wall_now = SystemTime::now();
        let wall_elapsed = wall_now.duration_since(self.last_wall_check).unwrap_or_default();
        let monotonic_elapsed = now.saturating_duration_since(self.last_monotonic_check);
        if system_suspended(wall_elapsed, monotonic_elapsed) && self.timer.running {
            self.timer.running = false;
            self.timer_dirty = true;
            self.error_message = Some(
                "Timer paused after sleep or system suspension. Press Start when ready.".to_owned(),
            );
        }
        self.last_wall_check = wall_now;
        self.last_monotonic_check = now;

        if let Some(pending) = &self.pending_power_action {
            if now >= pending.deadline {
                self.pending_power_action = None;
            }
        }
        if self.pending_power_action.is_some() {
            self.ctx.request_repaint_after(Duration::from_millis(500));
        }

        self.undo_deleted_notes.retain(|(when, _)| now.duration_since(*when) < UNDO_WINDOW);

        let completed = self.timer.update(&self.settings);
        if self.timer.running {
            self.timer_dirty = true;
        }

        let studied = self.timer.take_study_seconds();
        if studied > 0 {
            self.history.add_today(studied);
            self.history_dirty = true;
        }

        if self.history_dirty && self.last_history_flush.elapsed() >= HISTORY_FLUSH_INTERVAL {
            self.flush_history();
        }
        if self.timer_dirty && self.last_timer_flush.elapsed() >= Duration::from_secs(5) {
            self.flush_timer();
        }

        if let Some(mode) = completed {
            if self.settings.completion_sound {
                audio::play_completion_beep();
            }
            if self.settings.desktop_notifications {
                let message = match mode {
                    Mode::Study => "Focus session complete. Time for a break.",
                    Mode::ShortBreak | Mode::LongBreak => "Break complete. Time to focus.",
                };
                system::notify("Kronari", message);
            }
            if mode == Mode::Study {
                self.pending_note_duration = Some(u64::from(self.settings.study_minutes) * 60);
                self.history.add_completed_session_today();
                self.history_dirty = true;
            } else {
                self.task_prompt_pending = true;
            }
            self.flush_history();

            if self.settings.auto_start_next {
                self.timer.start();
            }

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

    pub fn flush_timer(&mut self) {
        if !self.timer_dirty {
            return;
        }
        match self.storage.save_timer(&self.timer.snapshot()) {
            Ok(()) => {
                self.timer_dirty = false;
                self.last_timer_flush = Instant::now();
            }
            Err(error) => {
                self.error_message = Some(format!("Could not save timer state: {error}"));
            }
        }
    }

    pub fn flush_all(&mut self) {
        self.flush_history();
        self.timer_dirty = true;
        self.flush_timer();
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
            "start" => {
                self.timer.start();
                self.should_restore_window = true;
            }
            "pause" => self.timer.pause(),
            "reset" => self.timer.reset(&self.settings),
            "skip" => {
                let studied = self.timer.take_study_seconds();
                if studied > 0 {
                    self.history.add_today(studied);
                    self.history_dirty = true;
                    self.flush_history();
                }
                self.timer.skip(&self.settings);
            }
            _ => return Err("Command must be start, pause, reset, or skip".to_owned()),
        }
        self.collect_study_time();
        self.timer_dirty = true;
        self.flush_timer();
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn update_settings(&mut self, mut settings: Settings) -> Result<(), String> {
        self.tick();
        settings.sanitize();
        // Preserve only the fields that callers (desktop and phone) never
        // Preserve fields controlled from the desktop or generated locally.
        settings.pin = self.settings.pin.clone();
        settings.port = self.settings.port;
        system::set_startup_enabled(settings.launch_at_startup)
            .map_err(|error| format!("Could not update startup settings: {error}"))?;
        self.storage
            .save_settings(&settings)
            .map_err(|error| format!("Could not save settings: {error}"))?;
        self.settings = settings;
        self.timer.set_settings(&self.settings);
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn save_weekly_reflection(
        &mut self,
        mut reflection: WeeklyReflection,
    ) -> Result<(), String> {
        reflection.week_start = WeeklyReflection::current_week_start();
        reflection.went_well = reflection.went_well.trim().to_owned();
        reflection.adjust_next_time = reflection.adjust_next_time.trim().to_owned();
        reflection.next_week_focus = reflection.next_week_focus.trim().to_owned();

        let previous = self.reflections.clone();
        self.reflections.upsert(reflection.clone());
        if let Err(error) = self.storage.save_reflections(&self.reflections) {
            self.reflections = previous;
            return Err(format!("Could not save weekly reflection: {error}"));
        }
        self.current_week_reflection = reflection;
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn save_daily_plan(&mut self, day: String, plan: String) -> Result<(), String> {
        let previous_history = self.history.clone();
        let was_dirty = self.history_dirty;
        self.history.set_plan_for(day, plan);
        self.history_dirty = true;
        match self.storage.save_history(&self.history) {
            Ok(()) => {
                self.history_dirty = false;
                self.last_history_flush = Instant::now();
                self.ctx.request_repaint();
                Ok(())
            }
            Err(error) => {
                self.history = previous_history;
                self.history_dirty = was_dirty;
                Err(format!("Could not save tomorrow’s plan: {error}"))
            }
        }
    }

    pub fn set_active_task(&mut self, task: String) -> Result<(), String> {
        let mut settings = self.settings.clone();
        settings.active_task = task.trim().to_owned();
        self.storage
            .save_settings(&settings)
            .map_err(|error| format!("Could not save active task: {error}"))?;
        self.settings = settings;
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn save_note(
        &mut self,
        accomplishment: String,
        subject: Option<String>,
        duration_seconds: u64,
    ) -> Result<String, String> {
        let accomplishment = accomplishment.trim().to_owned();
        if accomplishment.is_empty() {
            return Err("Note text cannot be empty".to_owned());
        }
        let subject = subject
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                let task = self.settings.active_task.trim();
                (!task.is_empty()).then(|| task.to_owned())
            });
        let id = generate_id();
        self.notes.entries.push(StudyNote {
            id: id.clone(),
            timestamp: Local::now().to_rfc3339(),
            duration_seconds,
            accomplishment,
            subject,
            updated_at: None,
        });
        if let Err(error) = self.storage.save_notes(&self.notes) {
            self.notes.entries.pop();
            return Err(format!("Could not save study note: {error}"));
        }
        self.ctx.request_repaint();
        Ok(id)
    }

    pub fn update_note(
        &mut self,
        id: &str,
        accomplishment: String,
        subject: Option<String>,
    ) -> Result<(), String> {
        let accomplishment = accomplishment.trim().to_owned();
        if accomplishment.is_empty() {
            return Err("Note text cannot be empty".to_owned());
        }
        let subject =
            subject.map(|value| value.trim().to_owned()).filter(|value| !value.is_empty());
        let Some(index) = self.notes.find_index(id) else {
            return Err("Note not found".to_owned());
        };

        let previous = self.notes.entries[index].clone();
        {
            let entry = &mut self.notes.entries[index];
            entry.accomplishment = accomplishment;
            entry.subject = subject;
            entry.updated_at = Some(Local::now().to_rfc3339());
        }

        if let Err(error) = self.storage.save_notes(&self.notes) {
            self.notes.entries[index] = previous;
            return Err(format!("Could not save study note: {error}"));
        }
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn delete_note(&mut self, id: &str) -> Result<(), String> {
        let Some(index) = self.notes.find_index(id) else {
            return Err("Note not found".to_owned());
        };
        let removed = self.notes.entries.remove(index);
        if let Err(error) = self.storage.save_notes(&self.notes) {
            self.notes.entries.insert(index, removed);
            return Err(format!("Could not delete study note: {error}"));
        }
        self.undo_deleted_notes.push((Instant::now(), removed));
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn undo_delete_note(&mut self) -> Result<(), String> {
        let now = Instant::now();
        let Some((when, note)) = self.undo_deleted_notes.pop() else {
            return Err("Nothing to undo".to_owned());
        };

        if now.duration_since(when) >= UNDO_WINDOW {
            self.undo_deleted_notes.push((when, note));
            return Err("Undo window expired".to_owned());
        }

        let mut trial = self.notes.entries.clone();
        trial.push(note.clone());
        if let Err(error) = self.storage.save_notes(&Notes { entries: trial }) {
            self.undo_deleted_notes.push((when, note));
            return Err(format!("Could not restore study note: {error}"));
        }
        self.notes.entries.push(note);
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn rotate_pin(&mut self) -> Result<(), String> {
        let new_pin =
            generate_pin().map_err(|error| format!("Could not generate a new PIN: {error}"))?;
        let mut settings = self.settings.clone();
        settings.pin = new_pin.clone();
        self.storage
            .save_settings(&settings)
            .map_err(|error| format!("Could not save the new PIN: {error}"))?;
        self.settings = settings;
        self.pin = new_pin;
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn request_new_port(&mut self) -> Result<(), String> {
        let probe = std::net::TcpListener::bind("0.0.0.0:0")
            .map_err(|error| format!("Could not probe for a free port: {error}"))?;
        let port = probe
            .local_addr()
            .map_err(|error| format!("Could not read the probed port: {error}"))?
            .port();
        drop(probe);

        let mut settings = self.settings.clone();
        settings.port = port;
        self.storage
            .save_settings(&settings)
            .map_err(|error| format!("Could not save the new port: {error}"))?;
        self.settings = settings;
        self.port = port;
        self.port_change_pending = true;
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn request_auto_port(&mut self) -> Result<(), String> {
        let mut settings = self.settings.clone();
        settings.port = 0;
        self.storage
            .save_settings(&settings)
            .map_err(|error| format!("Could not save the port setting: {error}"))?;
        self.settings = settings;
        self.port = 0;
        self.port_change_pending = true;
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn trigger_shutdown(&mut self) -> Result<(), String> {
        if !self.power_control_available() {
            return Err("Remote power control is disabled".to_owned());
        }
        if self.pending_power_action.is_some() {
            return Err("A power action is already pending".to_owned());
        }
        system::shutdown(POWER_ACTION_DELAY_SECS)
            .map_err(|error| format!("Could not schedule shutdown: {error}"))?;
        self.pending_power_action = Some(PendingPowerAction {
            action: PowerAction::Shutdown,
            deadline: Instant::now() + Duration::from_secs(u64::from(POWER_ACTION_DELAY_SECS)),
        });
        self.error_message = Some(format!(
            "Shutdown in {POWER_ACTION_DELAY_SECS}s — cancel from the phone or this app"
        ));
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn trigger_restart(&mut self) -> Result<(), String> {
        if !self.power_control_available() {
            return Err("Remote power control is disabled".to_owned());
        }
        if self.pending_power_action.is_some() {
            return Err("A power action is already pending".to_owned());
        }
        system::restart(POWER_ACTION_DELAY_SECS)
            .map_err(|error| format!("Could not schedule restart: {error}"))?;
        self.pending_power_action = Some(PendingPowerAction {
            action: PowerAction::Restart,
            deadline: Instant::now() + Duration::from_secs(u64::from(POWER_ACTION_DELAY_SECS)),
        });
        self.error_message = Some(format!(
            "Restart in {POWER_ACTION_DELAY_SECS}s — cancel from the phone or this app"
        ));
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn trigger_lock(&mut self) -> Result<(), String> {
        if !self.power_control_available() {
            return Err("Remote power control is disabled".to_owned());
        }
        system::lock_screen().map_err(|error| format!("Could not lock screen: {error}"))?;
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn cancel_power_action(&mut self) -> Result<(), String> {
        if self.pending_power_action.is_none() {
            return Err("No power action pending".to_owned());
        }
        system::cancel_pending().map_err(|error| format!("Could not cancel: {error}"))?;
        self.pending_power_action = None;
        self.error_message = Some("Power action cancelled".to_owned());
        self.ctx.request_repaint();
        Ok(())
    }

    pub fn reset_all_data(&mut self) -> Result<(), String> {
        let previous_history = self.history.clone();
        let previous_notes = self.notes.clone();
        let previous_reflections = self.reflections.clone();
        let previous_current_reflection = self.current_week_reflection.clone();

        self.history = History::default();
        self.notes = Notes::default();
        self.reflections = WeeklyReflections::default();
        self.current_week_reflection =
            self.reflections.for_week(&WeeklyReflection::current_week_start());
        self.history_dirty = true;
        self.undo_deleted_notes.clear();

        if let Err(error) = self.storage.save_history(&self.history) {
            self.history = previous_history;
            self.notes = previous_notes;
            self.reflections = previous_reflections;
            self.current_week_reflection = previous_current_reflection;
            return Err(format!("Could not reset history: {error}"));
        }
        if let Err(error) = self.storage.save_notes(&self.notes) {
            self.history = previous_history;
            self.notes = previous_notes;
            self.reflections = previous_reflections;
            self.current_week_reflection = previous_current_reflection;
            let _ = self.storage.save_history(&self.history);
            return Err(format!("Could not reset notes: {error}"));
        }
        if let Err(error) = self.storage.save_reflections(&self.reflections) {
            self.history = previous_history;
            self.notes = previous_notes;
            self.reflections = previous_reflections;
            self.current_week_reflection = previous_current_reflection;
            let _ = self.storage.save_history(&self.history);
            let _ = self.storage.save_notes(&self.notes);
            return Err(format!("Could not reset weekly reflections: {error}"));
        }

        self.history_dirty = false;
        self.last_history_flush = Instant::now();
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

fn load_with_warning<T>(result: std::io::Result<T>, name: &str, warnings: &mut Vec<String>) -> T
where
    T: Default,
{
    result.unwrap_or_else(|error| {
        warnings.push(format!("Could not load {name}: {error}"));
        T::default()
    })
}

fn system_suspended(wall_elapsed: Duration, monotonic_elapsed: Duration) -> bool {
    wall_elapsed.saturating_sub(monotonic_elapsed) > Duration::from_secs(5)
}

#[cfg(test)]
mod timekeeping_tests {
    use super::system_suspended;
    use std::time::Duration;

    #[test]
    fn a_long_ui_pause_is_not_mistaken_for_system_sleep() {
        assert!(!system_suspended(Duration::from_secs(60), Duration::from_secs(60)));
    }

    #[test]
    fn a_system_sleep_is_detected_when_wall_clock_advances_without_monotonic_time() {
        assert!(system_suspended(Duration::from_secs(60), Duration::ZERO));
    }
}

pub fn generate_pin() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes)?;
    let value = 10_000_000 + (u32::from_ne_bytes(bytes) % 90_000_000);
    Ok(value.to_string())
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

pub fn build_router(state: SharedState) -> Router {
    Router::new()
        .route("/", get(mobile_page))
        .route("/manifest.json", get(manifest))
        .route("/icon.svg", get(icon))
        .route("/status", get(status))
        .route("/command", post(command))
        .route("/settings", post(update_settings))
        .route("/notes", post(add_note))
        .route("/notes/update", post(update_note))
        .route("/notes/delete", post(delete_note))
        .route("/notes/undo-delete", post(undo_delete_note))
        .route("/export/history.csv", get(export_history_csv))
        .route("/export/notes.csv", get(export_notes_csv))
        .route("/export/all.csv", get(export_all_csv))
        .route("/export/all.json", get(export_all_json))
        .route("/import/preview", post(import_preview))
        .route("/import/apply", post(import_apply))
        .route("/reset", post(reset_all))
        .route("/system/shutdown", post(power_shutdown))
        .route("/system/restart", post(power_restart))
        .route("/system/lock", post(power_lock))
        .route("/system/cancel", post(power_cancel))
        .with_state(state)
}

pub fn start_server(state: SharedState) {
    thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(runtime) => runtime,
            Err(error) => {
                report_server_error(&state, format!("Could not start phone server: {error}"));
                return;
            }
        };

        let desired_port = match state.lock() {
            Ok(state) => state.port,
            Err(_) => 0,
        };

        let listener = match runtime.block_on(TcpListener::bind(("0.0.0.0", desired_port))) {
            Ok(listener) => listener,
            Err(error) => {
                report_server_error(
                    &state,
                    format!(
                        "Could not bind port {desired_port} ({error}); \
                         falling back to an OS-assigned port."
                    ),
                );
                match runtime.block_on(TcpListener::bind("0.0.0.0:0")) {
                    Ok(listener) => listener,
                    Err(error) => {
                        report_server_error(
                            &state,
                            format!("Could not bind phone server: {error}"),
                        );
                        return;
                    }
                }
            }
        };

        let port = match listener.local_addr() {
            Ok(address) => address.port(),
            Err(error) => {
                report_server_error(&state, format!("Could not read phone server port: {error}"));
                return;
            }
        };

        if let Ok(mut state) = state.lock() {
            state.local_url = local_url_for_port(port);
            if state.port != port {
                state.port = port;
                state.port_change_pending = false;
                let mut settings = state.settings.clone();
                settings.port = port;
                if let Err(error) = state.storage.save_settings(&settings) {
                    eprintln!("Could not persist the actual port: {error}");
                }
                state.settings = settings;
            }
            state.ctx.request_repaint();
        }

        let router = build_router(state.clone());

        eprintln!("Phone remote listening on 0.0.0.0:{port}");
        let result = runtime.block_on(async {
            axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await
        });
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
    ([(axum::http::header::CONTENT_TYPE, "image/svg+xml")], include_str!("../web/icon.svg"))
}

#[derive(Serialize)]
struct NoteSummary {
    id: String,
    timestamp: String,
    duration_seconds: u64,
    accomplishment: String,
    subject: Option<String>,
    updated_at: Option<String>,
}

impl From<&StudyNote> for NoteSummary {
    fn from(note: &StudyNote) -> Self {
        Self {
            id: note.id.clone(),
            timestamp: note.timestamp.clone(),
            duration_seconds: note.duration_seconds,
            accomplishment: note.accomplishment.clone(),
            subject: note.subject.clone(),
            updated_at: note.updated_at.clone(),
        }
    }
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
    current_streak: u32,
    weekly_streak_weeks: u32,
    weekly_focus_days: u32,
    weekly_goal_days: u32,
    completed_sessions: u32,
    today_sessions: u32,
    has_untracked_session_days: bool,
    notes: Vec<NoteSummary>,
    auto_start_next: bool,
    daily_goal_minutes: u32,
    daily_goal_progress: Option<f32>,
    break_suggestion: Option<String>,
    undo_available: bool,
    power_control_available: bool,
    remote_import: bool,
    pending_power_action: Option<PowerAction>,
    pending_power_seconds: Option<u32>,
    start_minimized: bool,
    close_to_tray: bool,
    active_task: String,
    desktop_notifications: bool,
    completion_sound: bool,
    launch_at_startup: bool,
}

async fn status(State(shared): State<SharedState>) -> Result<Json<StatusResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    state.tick();

    let today_seconds = state.history.today_seconds();
    let goal_minutes = state.settings.daily_goal_minutes;
    let goal_progress = if goal_minutes > 0 {
        Some((today_seconds as f32 / (goal_minutes as f32 * 60.0)).min(1.0))
    } else {
        None
    };

    let break_suggestion =
        breaks::current_suggestion(state.timer.mode, state.timer.elapsed_in_mode)
            .map(|s| s.to_owned());

    let undo_available = state.undo_available();
    let power_control_available = state.power_control_available();
    let remote_import = state.import_available();
    let pending_power_action = state.pending_power_action.as_ref().map(|p| p.action);
    let pending_power_seconds = state.pending_power_seconds();

    let notes: Vec<NoteSummary> =
        state.notes.entries.iter().rev().take(20).map(NoteSummary::from).collect();

    Ok(Json(StatusResponse {
        remaining_seconds: state.timer.remaining_seconds,
        mode: state.timer.mode,
        today_total_seconds: today_seconds,
        running: state.timer.running,
        study_minutes: state.settings.study_minutes,
        short_break_minutes: state.settings.short_break_minutes,
        long_break_minutes: state.settings.long_break_minutes,
        sessions_before_long_break: state.settings.sessions_before_long_break,
        current_streak: state.history.current_streak(),
        weekly_streak_weeks: state.history.weekly_streak(state.settings.weekly_goal_days),
        weekly_focus_days: state.history.weekly_focus_days(),
        weekly_goal_days: state.settings.weekly_goal_days,
        completed_sessions: state.timer.completed_study_sessions,
        today_sessions: state.history.today_sessions(),
        has_untracked_session_days: state.history.has_untracked_session_days(),
        notes,
        auto_start_next: state.settings.auto_start_next,
        daily_goal_minutes: goal_minutes,
        daily_goal_progress: goal_progress,
        break_suggestion,
        undo_available,
        power_control_available,
        remote_import,
        pending_power_action,
        pending_power_seconds,
        start_minimized: state.settings.start_minimized,
        close_to_tray: state.settings.close_to_tray,
        active_task: state.settings.active_task.clone(),
        desktop_notifications: state.settings.desktop_notifications,
        completion_sound: state.settings.completion_sound,
        launch_at_startup: state.settings.launch_at_startup,
    }))
}

#[derive(Deserialize)]
struct TokenQuery {
    token: String,
}

async fn export_history_csv(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    axum::extract::Query(query): axum::extract::Query<TokenQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &query.token)?;
    state.tick();
    let body = export::history_csv(&state.history);
    Ok((
        [(header::CONTENT_TYPE, "text/csv; charset=utf-8")],
        [(header::CONTENT_DISPOSITION, "attachment; filename=\"history.csv\"")],
        body,
    ))
}

async fn export_notes_csv(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    axum::extract::Query(query): axum::extract::Query<TokenQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &query.token)?;
    state.tick();
    let body = export::notes_csv(&state.notes);
    Ok((
        [(header::CONTENT_TYPE, "text/csv; charset=utf-8")],
        [(header::CONTENT_DISPOSITION, "attachment; filename=\"notes.csv\"")],
        body,
    ))
}

async fn export_all_csv(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    axum::extract::Query(query): axum::extract::Query<TokenQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &query.token)?;
    state.tick();
    let body = export::all_csv(&state.history, &state.notes);
    Ok((
        [(header::CONTENT_TYPE, "text/csv; charset=utf-8")],
        [(header::CONTENT_DISPOSITION, "attachment; filename=\"all.csv\"")],
        body,
    ))
}

async fn export_all_json(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    axum::extract::Query(query): axum::extract::Query<TokenQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &query.token)?;
    state.tick();
    let body = export::all_json(&state.history, &state.notes, &state.reflections);
    Ok((
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        [(header::CONTENT_DISPOSITION, "attachment; filename=\"all.json\"")],
        body,
    ))
}

#[derive(Deserialize)]
struct ImportRequest {
    token: String,
    file_contents: String,
}

async fn import_preview(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<ImportRequest>,
) -> Result<Json<ImportPreviewResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &request.token)?;
    if !state.import_available() {
        return Err(ApiError::forbidden("Remote import is disabled"));
    }
    let bundle =
        crate::import::parse(request.file_contents.as_bytes()).map_err(ApiError::bad_request)?;
    let preview = crate::import::preview(&bundle, &state.history, &state.notes);
    Ok(Json(ImportPreviewResponse {
        version: preview.version,
        exported_at: preview.exported_at,
        history_days: preview.history_days,
        history_seconds: preview.history_seconds,
        notes: preview.notes,
        overlapping_days: preview.overlapping_days,
        overlapping_note_ids: preview.overlapping_note_ids,
    }))
}

#[derive(Serialize)]
struct ImportPreviewResponse {
    version: u32,
    exported_at: String,
    history_days: usize,
    history_seconds: u64,
    notes: usize,
    overlapping_days: Vec<String>,
    overlapping_note_ids: Vec<String>,
}

#[derive(Deserialize)]
struct ImportApplyRequest {
    token: String,
    file_contents: String,
    mode: String,
}

async fn import_apply(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<ImportApplyRequest>,
) -> Result<Json<ImportApplyResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &request.token)?;
    if !state.import_available() {
        return Err(ApiError::forbidden("Remote import is disabled"));
    }
    let mode = match request.mode.as_str() {
        "replace" => crate::import::Mode::Replace,
        "merge" => crate::import::Mode::Merge,
        other => {
            return Err(ApiError::bad_request(format!(
                "mode must be 'replace' or 'merge', got {other:?}"
            )));
        }
    };
    let bundle =
        crate::import::parse(request.file_contents.as_bytes()).map_err(ApiError::bad_request)?;

    let previous_history = state.history.clone();
    let previous_notes = state.notes.clone();
    let previous_reflections = state.reflections.clone();

    let mut history = std::mem::take(&mut state.history);
    let mut notes = std::mem::take(&mut state.notes);
    let mut reflections = std::mem::take(&mut state.reflections);
    let report = crate::import::apply_with_reflections(
        bundle,
        &mut history,
        &mut notes,
        &mut reflections,
        mode,
    );
    state.history = history;
    state.notes = notes;
    state.reflections = reflections;

    if let Err(error) = state.storage.save_history(&state.history) {
        state.history = previous_history;
        state.notes = previous_notes;
        state.reflections = previous_reflections;
        let _ = state.storage.save_history(&state.history);
        let _ = state.storage.save_notes(&state.notes);
        return Err(ApiError::internal(format!("Could not save history: {error}")));
    }
    if let Err(error) = state.storage.save_notes(&state.notes) {
        state.history = previous_history;
        state.notes = previous_notes;
        state.reflections = previous_reflections;
        let _ = state.storage.save_history(&state.history);
        let _ = state.storage.save_notes(&state.notes);
        return Err(ApiError::internal(format!("Could not save notes: {error}")));
    }
    if let Err(error) = state.storage.save_reflections(&state.reflections) {
        state.history = previous_history;
        state.notes = previous_notes;
        state.reflections = previous_reflections;
        let _ = state.storage.save_history(&state.history);
        let _ = state.storage.save_notes(&state.notes);
        let _ = state.storage.save_reflections(&state.reflections);
        return Err(ApiError::internal(format!("Could not save weekly reflections: {error}")));
    }
    state.current_week_reflection =
        state.reflections.for_week(&WeeklyReflection::current_week_start());

    state.ctx.request_repaint();
    Ok(Json(ImportApplyResponse {
        ok: true,
        history_days_added: report.history_days_added,
        history_days_updated: report.history_days_updated,
        notes_added: report.notes_added,
        notes_skipped: report.notes_skipped,
    }))
}

#[derive(Serialize)]
struct ImportApplyResponse {
    ok: bool,
    history_days_added: usize,
    history_days_updated: usize,
    notes_added: usize,
    notes_skipped: usize,
}

async fn reset_all(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<TokenOnlyRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &request.token)?;
    state.reset_all_data().map_err(ApiError::internal)?;
    Ok(Json(MessageResponse { ok: true, warning: None }))
}

#[derive(Deserialize)]
struct CommandRequest {
    token: String,
    command: String,
}

async fn command(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<CommandRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &request.token)?;
    state.execute_command(&request.command).map_err(ApiError::bad_request)?;
    Ok(Json(MessageResponse { ok: true, warning: None }))
}

#[derive(Deserialize)]
struct SettingsRequest {
    token: String,
    study_minutes: u32,
    short_break_minutes: u32,
    long_break_minutes: u32,
    sessions_before_long_break: u32,
    #[serde(default)]
    auto_start_next: bool,
    #[serde(default)]
    daily_goal_minutes: u32,
    #[serde(default)]
    weekly_goal_days: Option<u32>,
    #[serde(default)]
    remote_power_control: bool,
    #[serde(default)]
    remote_import: bool,
    #[serde(default)]
    start_minimized: bool,
    #[serde(default)]
    close_to_tray: bool,
    #[serde(default)]
    active_task: String,
    #[serde(default)]
    desktop_notifications: bool,
    #[serde(default)]
    completion_sound: bool,
    #[serde(default)]
    launch_at_startup: bool,
}

async fn update_settings(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<SettingsRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &request.token)?;
    let settings = Settings {
        study_minutes: request.study_minutes,
        short_break_minutes: request.short_break_minutes,
        long_break_minutes: request.long_break_minutes,
        sessions_before_long_break: request.sessions_before_long_break,
        pin: state.settings.pin.clone(),
        port: state.settings.port,
        auto_start_next: request.auto_start_next,
        daily_goal_minutes: request.daily_goal_minutes,
        weekly_goal_days: request.weekly_goal_days.unwrap_or(state.settings.weekly_goal_days),
        remote_power_control: request.remote_power_control,
        remote_import: request.remote_import,
        start_minimized: request.start_minimized,
        close_to_tray: request.close_to_tray,
        active_task: request.active_task,
        desktop_notifications: request.desktop_notifications,
        completion_sound: request.completion_sound,
        launch_at_startup: request.launch_at_startup,
        compact_timer: state.settings.compact_timer,
        always_on_top: state.settings.always_on_top,
        pause_when_minimized: state.settings.pause_when_minimized,
        onboarding_complete: state.settings.onboarding_complete,
    };
    state.update_settings(settings).map_err(ApiError::internal)?;
    Ok(Json(MessageResponse { ok: true, warning: None }))
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
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<NoteRequest>,
) -> Result<Json<NoteCreatedResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &request.token)?;
    let subject = if request.subject.trim().is_empty() { None } else { Some(request.subject) };
    let id = state.save_note(request.accomplishment, subject, 0).map_err(ApiError::bad_request)?;
    Ok(Json(NoteCreatedResponse { ok: true, id }))
}

#[derive(Deserialize)]
struct NoteUpdateRequest {
    token: String,
    id: String,
    accomplishment: String,
    #[serde(default)]
    subject: String,
}

async fn update_note(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<NoteUpdateRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &request.token)?;
    let subject = if request.subject.trim().is_empty() { None } else { Some(request.subject) };
    state
        .update_note(&request.id, request.accomplishment, subject)
        .map_err(ApiError::bad_request)?;
    Ok(Json(MessageResponse { ok: true, warning: None }))
}

#[derive(Deserialize)]
struct NoteDeleteRequest {
    token: String,
    id: String,
}

async fn delete_note(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<NoteDeleteRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &request.token)?;
    state.delete_note(&request.id).map_err(ApiError::bad_request)?;
    Ok(Json(MessageResponse { ok: true, warning: None }))
}

#[derive(Deserialize)]
struct TokenOnlyRequest {
    token: String,
}

async fn undo_delete_note(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<TokenOnlyRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &request.token)?;
    state.undo_delete_note().map_err(ApiError::bad_request)?;
    Ok(Json(MessageResponse { ok: true, warning: None }))
}

#[derive(Deserialize)]
struct PowerActionRequest {
    token: String,
    #[serde(default)]
    confirm: bool,
}

async fn power_shutdown(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<PowerActionRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &request.token)?;
    if !state.power_control_available() {
        return Err(ApiError::forbidden("Remote power control is disabled"));
    }
    if !request.confirm {
        return Err(ApiError::bad_request("Missing confirm=true".to_owned()));
    }
    state.trigger_shutdown().map_err(ApiError::internal)?;
    Ok(Json(MessageResponse { ok: true, warning: None }))
}

async fn power_restart(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<PowerActionRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &request.token)?;
    if !state.power_control_available() {
        return Err(ApiError::forbidden("Remote power control is disabled"));
    }
    if !request.confirm {
        return Err(ApiError::bad_request("Missing confirm=true".to_owned()));
    }
    state.trigger_restart().map_err(ApiError::internal)?;
    Ok(Json(MessageResponse { ok: true, warning: None }))
}

async fn power_lock(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<TokenOnlyRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &request.token)?;
    if !state.power_control_available() {
        return Err(ApiError::forbidden("Remote power control is disabled"));
    }
    state.trigger_lock().map_err(ApiError::internal)?;
    Ok(Json(MessageResponse { ok: true, warning: None }))
}

async fn power_cancel(
    State(shared): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<TokenOnlyRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let mut state = shared.lock().map_err(|_| ApiError::internal("App state is unavailable"))?;
    verify_token(&mut state, addr.ip(), &request.token)?;
    state.cancel_power_action().map_err(ApiError::bad_request)?;
    Ok(Json(MessageResponse { ok: true, warning: None }))
}

fn verify_token(state: &mut AppState, ip: IpAddr, token: &str) -> Result<(), ApiError> {
    let now = Instant::now();

    if let Some(record) = state.auth.failures.get(&ip) {
        if record.locked_until.is_some_and(|until| now < until) {
            return Err(ApiError::rate_limited(
                "Too many incorrect PIN attempts from this device. Try again in one minute.",
            ));
        }
    }

    if state.pin == token {
        state.auth.record_success(ip);
        Ok(())
    } else {
        state.auth.record_failure(ip, now);
        Err(ApiError::unauthorized("Incorrect PIN"))
    }
}

#[derive(Serialize)]
struct MessageResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

#[derive(Serialize)]
struct NoteCreatedResponse {
    ok: bool,
    id: String,
}

struct ApiError(StatusCode, String);

impl ApiError {
    fn bad_request(message: String) -> Self {
        Self(StatusCode::BAD_REQUEST, message)
    }

    fn unauthorized(message: &str) -> Self {
        Self(StatusCode::UNAUTHORIZED, message.to_owned())
    }

    fn forbidden(message: &str) -> Self {
        Self(StatusCode::FORBIDDEN, message.to_owned())
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
