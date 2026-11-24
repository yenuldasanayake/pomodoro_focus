use chrono::{Datelike, Duration, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub study_minutes: u32,
    pub short_break_minutes: u32,
    pub long_break_minutes: u32,
    pub sessions_before_long_break: u32,
    pub pin: String,
    pub port: u16,
    pub auto_start_next: bool,
    pub daily_goal_minutes: u32,
    #[serde(default = "default_weekly_goal_days")]
    pub weekly_goal_days: u32,
    pub remote_power_control: bool,
    pub remote_import: bool,
    pub start_minimized: bool,
    pub close_to_tray: bool,
    #[serde(default)]
    pub active_task: String,
    #[serde(default)]
    pub desktop_notifications: bool,
    #[serde(default)]
    pub completion_sound: bool,
    #[serde(default)]
    pub launch_at_startup: bool,
    #[serde(default)]
    pub compact_timer: bool,
    #[serde(default)]
    pub always_on_top: bool,
    #[serde(default = "legacy_setup_complete")]
    pub onboarding_complete: bool,
}

fn default_weekly_goal_days() -> u32 {
    5
}

fn legacy_setup_complete() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            study_minutes: 25,
            short_break_minutes: 5,
            long_break_minutes: 15,
            sessions_before_long_break: 4,
            pin: String::new(),
            port: 0,
            auto_start_next: false,
            daily_goal_minutes: 0,
            weekly_goal_days: default_weekly_goal_days(),
            remote_power_control: false,
            remote_import: false,
            start_minimized: false,
            close_to_tray: false,
            active_task: String::new(),
            desktop_notifications: true,
            completion_sound: true,
            launch_at_startup: false,
            compact_timer: false,
            always_on_top: false,
            onboarding_complete: false,
        }
    }
}

impl Settings {
    pub fn sanitize(&mut self) {
        self.study_minutes = self.study_minutes.clamp(1, 180);
        self.short_break_minutes = self.short_break_minutes.clamp(1, 60);
        self.long_break_minutes = self.long_break_minutes.clamp(1, 120);
        self.sessions_before_long_break = self.sessions_before_long_break.clamp(1, 12);
        if self.port < 1024 {
            self.port = 0;
        }
        if !self.pin.is_empty()
            && (self.pin.len() != 8 || !self.pin.chars().all(|c| c.is_ascii_digit()))
        {
            self.pin.clear();
        }
        if self.daily_goal_minutes > 0 {
            self.daily_goal_minutes = self.daily_goal_minutes.clamp(15, 720);
        }
        self.weekly_goal_days = self.weekly_goal_days.clamp(1, 7);
    }
}

#[cfg(test)]
mod settings_tests {
    use super::Settings;

    #[test]
    fn older_settings_keep_optional_controls_disabled() {
        let settings: Settings = serde_json::from_str(
            r#"{"study_minutes":25,"short_break_minutes":5,"long_break_minutes":15,"sessions_before_long_break":4}"#,
        )
        .unwrap();

        assert!(settings.pin.is_empty());
        assert_eq!(settings.port, 0);
        assert!(!settings.auto_start_next);
        assert_eq!(settings.daily_goal_minutes, 0);
        assert_eq!(settings.weekly_goal_days, 5);
        assert!(!settings.remote_power_control);
        assert!(!settings.remote_import);
        assert!(!settings.start_minimized);
        assert!(!settings.close_to_tray);
        assert!(!settings.compact_timer);
        assert!(!settings.always_on_top);
    }

    #[test]
    fn pin_and_port_round_trip() {
        let settings = Settings { pin: "12345678".to_owned(), port: 51234, ..Settings::default() };
        let saved = serde_json::to_string(&settings).unwrap();
        let loaded: Settings = serde_json::from_str(&saved).unwrap();
        assert_eq!(loaded.pin, "12345678");
        assert_eq!(loaded.port, 51234);
    }

    #[test]
    fn sanitize_rejects_malformed_pin_and_privileged_port() {
        let mut settings = Settings { pin: "abc".to_owned(), port: 80, ..Settings::default() };
        settings.sanitize();
        assert!(settings.pin.is_empty());
        assert_eq!(settings.port, 0);
    }

    #[test]
    fn sanitize_accepts_valid_pin_and_high_port() {
        let mut settings =
            Settings { pin: "98765432".to_owned(), port: 51234, ..Settings::default() };
        settings.sanitize();
        assert_eq!(settings.pin, "98765432");
        assert_eq!(settings.port, 51234);
    }

    #[test]
    fn auto_start_and_goal_default_off() {
        let settings: Settings = serde_json::from_str(r#"{}"#).unwrap();
        assert!(!settings.auto_start_next);
        assert_eq!(settings.daily_goal_minutes, 0);
    }

    #[test]
    fn goal_is_clamped_when_nonzero() {
        let mut settings = Settings { daily_goal_minutes: 5, ..Settings::default() };
        settings.sanitize();
        assert_eq!(settings.daily_goal_minutes, 15);

        let mut settings = Settings { daily_goal_minutes: 5000, ..Settings::default() };
        settings.sanitize();
        assert_eq!(settings.daily_goal_minutes, 720);

        let mut settings = Settings { daily_goal_minutes: 0, ..Settings::default() };
        settings.sanitize();
        assert_eq!(settings.daily_goal_minutes, 0);
    }

    #[test]
    fn remote_power_control_defaults_off() {
        let settings: Settings = serde_json::from_str(r#"{}"#).unwrap();
        assert!(!settings.remote_power_control);
    }

    #[test]
    fn remote_import_defaults_off() {
        let settings: Settings = serde_json::from_str(r#"{}"#).unwrap();
        assert!(!settings.remote_import);
    }

    #[test]
    fn tray_settings_default_off() {
        let settings: Settings = serde_json::from_str(r#"{}"#).unwrap();
        assert!(!settings.start_minimized);
        assert!(!settings.close_to_tray);
    }

    #[test]
    fn startup_setting_defaults_off() {
        let settings: Settings = serde_json::from_str(r#"{}"#).unwrap();
        assert!(!settings.launch_at_startup);
    }

    #[test]
    fn weekly_goal_defaults_to_five_and_is_clamped() {
        let settings: Settings = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(settings.weekly_goal_days, 5);

        let mut settings = Settings { weekly_goal_days: 0, ..Settings::default() };
        settings.sanitize();
        assert_eq!(settings.weekly_goal_days, 1);

        settings.weekly_goal_days = 8;
        settings.sanitize();
        assert_eq!(settings.weekly_goal_days, 7);
    }

    #[test]
    fn existing_settings_skip_first_run_setup() {
        let settings: Settings = serde_json::from_str(r#"{"study_minutes":25}"#).unwrap();
        assert!(settings.onboarding_complete);
        assert!(!Settings::default().onboarding_complete);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub version: u32,
    pub daily_seconds: BTreeMap<String, u64>,
    #[serde(default)]
    pub daily_sessions: BTreeMap<String, u32>,
    #[serde(default)]
    pub daily_plans: BTreeMap<String, String>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            daily_seconds: BTreeMap::new(),
            daily_sessions: BTreeMap::new(),
            daily_plans: BTreeMap::new(),
        }
    }
}

impl History {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn today_seconds(&self) -> u64 {
        self.daily_seconds.get(&Local::now().date_naive().to_string()).copied().unwrap_or_default()
    }

    pub fn add_today(&mut self, seconds: u64) {
        let day = Local::now().date_naive().to_string();
        *self.daily_seconds.entry(day).or_default() += seconds;
    }

    pub fn add_completed_session_today(&mut self) {
        let day = Local::now().date_naive().to_string();
        *self.daily_sessions.entry(day).or_default() += 1;
    }

    pub fn today_sessions(&self) -> u32 {
        self.daily_sessions.get(&Local::now().date_naive().to_string()).copied().unwrap_or_default()
    }

    pub fn plan_for(&self, day: &str) -> Option<&str> {
        self.daily_plans.get(day).map(String::as_str)
    }

    pub fn set_plan_for(&mut self, day: String, plan: String) {
        let plan = plan.trim();
        if plan.is_empty() {
            self.daily_plans.remove(&day);
        } else {
            self.daily_plans.insert(day, plan.to_owned());
        }
    }

    pub fn session_counts_started_on(&self) -> Option<&str> {
        self.daily_sessions.keys().next().map(String::as_str)
    }

    pub fn has_untracked_session_days(&self) -> bool {
        match (self.daily_seconds.keys().next(), self.session_counts_started_on()) {
            (Some(first_study_day), Some(first_counted_day)) => {
                first_study_day.as_str() < first_counted_day
            }
            (Some(_), None) => true,
            _ => false,
        }
    }

    pub fn current_week_seconds(&self) -> u64 {
        let today = Local::now().date_naive();
        let monday = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
        (0..=today.signed_duration_since(monday).num_days())
            .map(|offset| {
                let day = monday + Duration::days(offset);
                self.daily_seconds.get(&day.to_string()).copied().unwrap_or_default()
            })
            .sum()
    }

    pub fn best_day(&self) -> Option<(&str, u64)> {
        self.daily_seconds
            .iter()
            .max_by_key(|(_, seconds)| *seconds)
            .map(|(day, seconds)| (day.as_str(), *seconds))
    }

    pub fn period_seconds(&self, days: u32) -> u64 {
        let today = Local::now().date_naive();
        (0..days)
            .map(|offset| {
                let day = today - Duration::days(i64::from(offset));
                self.daily_seconds.get(&day.to_string()).copied().unwrap_or_default()
            })
            .sum()
    }

    pub fn current_streak(&self) -> u32 {
        let today = Local::now().date_naive();
        let mut streak = 0;
        let mut day = today;
        if !self.has_study_on(today) {
            day = today.pred_opt().unwrap_or(today);
        }
        while self.has_study_on(day) {
            streak += 1;
            let Some(previous) = day.pred_opt() else {
                break;
            };
            day = previous;
        }
        streak
    }

    pub fn weekly_focus_days(&self) -> u32 {
        let today = Local::now().date_naive();
        let week_start = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
        self.focus_days_between(week_start, today)
    }

    pub fn weekly_streak(&self, weekly_goal_days: u32) -> u32 {
        let today = Local::now().date_naive();
        let target = weekly_goal_days.clamp(1, 7);
        let current_week_start =
            today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
        let current_week_met = self.focus_days_between(current_week_start, today) >= target;
        let mut week_start = if current_week_met {
            current_week_start
        } else {
            current_week_start - Duration::days(7)
        };
        let mut streak = 0;
        loop {
            let week_end = week_start + Duration::days(6);
            if self.focus_days_between(week_start, week_end) < target {
                break;
            }
            streak += 1;
            week_start -= Duration::days(7);
        }
        streak
    }

    fn focus_days_between(&self, start: NaiveDate, end: NaiveDate) -> u32 {
        (0..=end.signed_duration_since(start).num_days())
            .filter(|offset| {
                let day = start + Duration::days(*offset);
                self.daily_seconds.get(&day.to_string()).is_some_and(|seconds| *seconds > 0)
            })
            .count() as u32
    }

    fn has_study_on(&self, day: NaiveDate) -> bool {
        self.daily_seconds.get(&day.to_string()).is_some_and(|seconds| *seconds > 0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudyNote {
    #[serde(default)]
    pub id: String,
    pub timestamp: String,
    pub duration_seconds: u64,
    pub accomplishment: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Notes {
    pub entries: Vec<StudyNote>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WeeklyReflections {
    pub entries: Vec<WeeklyReflection>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeeklyReflection {
    pub week_start: String,
    pub went_well: String,
    pub adjust_next_time: String,
    pub next_week_focus: String,
}

impl WeeklyReflection {
    pub fn current_week_start() -> String {
        let today = Local::now().date_naive();
        let monday = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
        monday.to_string()
    }
}

impl WeeklyReflections {
    pub fn for_week(&self, week_start: &str) -> WeeklyReflection {
        self.entries.iter().find(|entry| entry.week_start == week_start).cloned().unwrap_or_else(
            || WeeklyReflection {
                week_start: week_start.to_owned(),
                ..WeeklyReflection::default()
            },
        )
    }

    pub fn upsert(&mut self, reflection: WeeklyReflection) {
        if let Some(existing) =
            self.entries.iter_mut().find(|entry| entry.week_start == reflection.week_start)
        {
            *existing = reflection;
        } else {
            self.entries.push(reflection);
        }
    }
}

impl Notes {
    pub fn ensure_ids(&mut self) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            if entry.id.is_empty() {
                entry.id = generate_id();
                changed = true;
            }
        }
        changed
    }

    pub fn find_index(&self, id: &str) -> Option<usize> {
        self.entries.iter().position(|n| n.id == id)
    }
}

pub fn generate_id() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        let now = Local::now().timestamp_nanos_opt().unwrap_or_default();
        return format!("{now:032x}");
    }
    let mut out = String::with_capacity(32);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod notes_tests {
    use super::*;

    fn sample_note(id: &str) -> StudyNote {
        StudyNote {
            id: id.to_owned(),
            timestamp: "2026-10-16T10:00:00Z".to_owned(),
            duration_seconds: 1500,
            accomplishment: "Wrote tests".to_owned(),
            subject: Some("Rust".to_owned()),
            updated_at: None,
        }
    }

    #[test]
    fn ensure_ids_fills_missing_ids() {
        let mut notes = Notes {
            entries: vec![StudyNote { id: String::new(), ..sample_note("") }, sample_note("kept")],
        };

        assert!(notes.ensure_ids());
        assert!(!notes.entries[0].id.is_empty());
        assert_eq!(notes.entries[1].id, "kept");
        assert!(!notes.ensure_ids(), "second pass should be a no-op");
    }

    #[test]
    fn find_index_locates_by_id() {
        let notes = Notes { entries: vec![sample_note("a"), sample_note("b")] };
        assert_eq!(notes.find_index("a"), Some(0));
        assert_eq!(notes.find_index("b"), Some(1));
        assert_eq!(notes.find_index("missing"), None);
    }

    #[test]
    fn legacy_note_without_id_deserializes() {
        let note: StudyNote = serde_json::from_str(
            r#"{"timestamp":"2026-10-16T10:00:00Z","duration_seconds":1500,"accomplishment":"x"}"#,
        )
        .unwrap();
        assert!(note.id.is_empty());
        assert!(note.updated_at.is_none());
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;
    use chrono::Duration;

    fn history_with_days(days: &[i64]) -> History {
        let today = Local::now().date_naive();
        let mut history = History::default();
        for &offset in days {
            let day = today + Duration::days(offset);
            history.daily_seconds.insert(day.to_string(), 60);
        }
        history
    }

    #[test]
    fn streak_zero_when_empty() {
        assert_eq!(History::default().current_streak(), 0);
    }

    #[test]
    fn streak_one_for_today_only() {
        assert_eq!(history_with_days(&[0]).current_streak(), 1);
    }

    #[test]
    fn streak_one_for_yesterday_only() {
        assert_eq!(history_with_days(&[-1]).current_streak(), 1);
    }

    #[test]
    fn streak_zero_for_gap_day_only() {
        assert_eq!(history_with_days(&[-2]).current_streak(), 0);
    }

    #[test]
    fn streak_two_for_today_and_yesterday() {
        assert_eq!(history_with_days(&[0, -1]).current_streak(), 2);
    }

    #[test]
    fn streak_stops_at_gap() {
        assert_eq!(history_with_days(&[0, -1, -3]).current_streak(), 2);
    }

    #[test]
    fn zero_seconds_does_not_count_as_study() {
        let today = Local::now().date_naive();
        let mut history = History::default();
        history.daily_seconds.insert(today.to_string(), 0);
        assert_eq!(history.current_streak(), 0);
    }

    #[test]
    fn completed_sessions_are_counted_for_today() {
        let mut history = History::default();
        history.add_completed_session_today();
        history.add_completed_session_today();
        assert_eq!(history.today_sessions(), 2);
    }

    #[test]
    fn weekly_progress_counts_active_days_and_allows_rest_days() {
        let today = Local::now().date_naive();
        let week_start = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
        let previous_week = week_start - Duration::days(7);
        let mut history = History::default();
        for offset in 0..5 {
            let day = previous_week + Duration::days(offset);
            history.daily_seconds.insert(day.to_string(), 60);
        }
        history.daily_seconds.insert(week_start.to_string(), 60);
        if today != week_start {
            history.daily_seconds.insert(today.to_string(), 60);
        }

        assert_eq!(history.weekly_focus_days(), if today == week_start { 1 } else { 2 });
        assert_eq!(history.weekly_streak(5), 1);
    }

    #[test]
    fn unfinished_current_week_does_not_break_a_met_weekly_goal() {
        let today = Local::now().date_naive();
        let current_week =
            today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
        let previous_week = current_week - Duration::days(7);
        let mut history = History::default();
        for offset in 0..5 {
            let day = previous_week + Duration::days(offset);
            history.daily_seconds.insert(day.to_string(), 60);
        }

        assert_eq!(history.weekly_streak(5), 1);
    }

    #[test]
    fn missed_full_week_ends_the_weekly_streak() {
        let today = Local::now().date_naive();
        let current_week =
            today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
        let two_weeks_ago = current_week - Duration::days(14);
        let previous_week = current_week - Duration::days(7);
        let mut history = History::default();
        for offset in 0..5 {
            history.daily_seconds.insert((two_weeks_ago + Duration::days(offset)).to_string(), 60);
        }
        for offset in 0..4 {
            history.daily_seconds.insert((previous_week + Duration::days(offset)).to_string(), 60);
        }

        assert_eq!(history.weekly_streak(5), 0);
    }

    #[test]
    fn current_week_can_extend_the_weekly_streak_once_goal_is_met() {
        let today = Local::now().date_naive();
        let current_week =
            today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
        let previous_week = current_week - Duration::days(7);
        let mut history = History::default();
        history.daily_seconds.insert(today.to_string(), 60);
        history.daily_seconds.insert(previous_week.to_string(), 60);

        assert_eq!(history.weekly_streak(1), 2);
    }

    #[test]
    fn daily_plans_are_trimmed_and_empty_plans_are_removed() {
        let mut history = History::default();
        history.set_plan_for("2026-11-23".to_owned(), "  Start chapter 3  ".to_owned());
        assert_eq!(history.plan_for("2026-11-23"), Some("Start chapter 3"));

        history.set_plan_for("2026-11-23".to_owned(), "  ".to_owned());
        assert_eq!(history.plan_for("2026-11-23"), None);
    }

    #[test]
    fn daily_plans_round_trip_in_history_json() {
        let mut history = History::default();
        history.set_plan_for("2026-11-23".to_owned(), "Start chapter 3".to_owned());

        let json = serde_json::to_string(&history).unwrap();
        let restored: History = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.plan_for("2026-11-23"), Some("Start chapter 3"));
    }

    #[test]
    fn old_study_days_are_marked_as_missing_session_counts() {
        let mut history = History::default();
        history.daily_seconds.insert("2026-01-01".to_owned(), 3600);
        history.daily_sessions.insert("2026-01-02".to_owned(), 1);
        assert!(history.has_untracked_session_days());
    }

    #[test]
    fn older_history_without_session_counts_still_loads() {
        let history: History = serde_json::from_str(r#"{"version":1,"daily_seconds":{}}"#).unwrap();
        assert!(history.daily_sessions.is_empty());
        assert!(history.daily_plans.is_empty());
    }
}

#[derive(Debug, Clone)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn check_writable(&self) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        let probe = self.root.join(".pomodoro-write-probe");
        fs::write(&probe, b"ok").map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "The app directory {} is not writable ({error}). \
                     Notes and history will not save.",
                    self.root.display()
                ),
            )
        })?;
        let _ = fs::remove_file(&probe);
        Ok(())
    }

    fn diagnose_write_failure(&self, path: &Path, error: &io::Error) -> String {
        let parent = path.parent().unwrap_or(&self.root);
        if !parent.exists() {
            return format!(
                "Could not save {}: parent directory {} does not exist. ({error})",
                path.display(),
                parent.display()
            );
        }
        match fs::metadata(path) {
            Ok(meta) if meta.permissions().readonly() => format!(
                "Could not save {}: the file is marked read-only. \
                 On Windows, run `attrib -R \"{}\"` and try again. ({error})",
                path.display(),
                path.display()
            ),
            Ok(_) => format!(
                "Could not save {}: {error}. The file may be open in another program \
                 (editor, antivirus scanner, backup tool).",
                path.display()
            ),
            Err(meta_error) if meta_error.kind() == io::ErrorKind::NotFound => format!(
                "Could not save {}: {error}. The file does not exist yet, but the \
                 directory {} might be read-only.",
                path.display(),
                parent.display()
            ),
            Err(meta_error) => format!(
                "Could not save {}: {error} (also could not stat the file: {meta_error})",
                path.display()
            ),
        }
    }

    pub fn load_settings(&self) -> io::Result<Settings> {
        let mut settings: Settings = self.load_or_default("settings.json")?;
        settings.sanitize();
        Ok(settings)
    }

    pub fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        self.save("settings.json", settings)
    }

    pub fn load_history(&self) -> io::Result<History> {
        let mut history: History = self.load_or_default(Path::new("data").join("history.json"))?;
        if history.version == 0 {
            history.version = History::CURRENT_VERSION;
        }
        Ok(history)
    }

    pub fn save_history(&self, history: &History) -> io::Result<()> {
        self.save(Path::new("data").join("history.json"), history)
    }

    pub fn load_notes(&self) -> io::Result<Notes> {
        let mut notes: Notes = self.load_or_default(Path::new("data").join("notes.json"))?;
        if notes.ensure_ids() {
            if let Err(error) = self.save_notes(&notes) {
                eprintln!("Could not persist note ids: {error}");
            }
        }
        Ok(notes)
    }

    pub fn save_notes(&self, notes: &Notes) -> io::Result<()> {
        self.save(Path::new("data").join("notes.json"), notes)
    }

    pub fn load_reflections(&self) -> io::Result<WeeklyReflections> {
        self.load_or_default(Path::new("data").join("reflections.json"))
    }

    pub fn save_reflections(&self, reflections: &WeeklyReflections) -> io::Result<()> {
        self.save(Path::new("data").join("reflections.json"), reflections)
    }

    pub fn load_timer(&self) -> io::Result<crate::timer::TimerSnapshot> {
        self.load_or_default("timer.json")
    }

    pub fn save_timer(&self, snapshot: &crate::timer::TimerSnapshot) -> io::Result<()> {
        self.save("timer.json", snapshot)
    }

    fn load_or_default<T>(&self, relative: impl AsRef<Path>) -> io::Result<T>
    where
        T: for<'de> Deserialize<'de> + Default,
    {
        let path = self.root.join(relative);
        match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(value) => Ok(value),
                Err(error) => {
                    eprintln!("Could not parse {}: {error}; using defaults", path.display());
                    Ok(T::default())
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(T::default()),
            Err(error) => Err(error),
        }
    }

    fn save<T: Serialize>(&self, relative: impl AsRef<Path>, value: &T) -> io::Result<()> {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("Could not create {}: {error}", parent.display()),
                )
            })?;
        }

        let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;

        let tmp = path.with_extension(format!(
            "tmp-{}-{}",
            std::process::id(),
            Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));

        if let Err(error) = fs::write(&tmp, &bytes) {
            let _ = fs::remove_file(&tmp);
            return Err(io::Error::new(
                error.kind(),
                format!("Could not write {}: {error}", tmp.display()),
            ));
        }

        if let Ok(file) = fs::OpenOptions::new().write(true).open(&tmp) {
            let _ = file.sync_all();
        }

        if let Err(error) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            let message = self.diagnose_write_failure(&path, &error);
            return Err(io::Error::new(error.kind(), message));
        }

        Ok(())
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    use std::env;

    fn temp_root(name: &str) -> PathBuf {
        let mut path = env::temp_dir();
        let unique = format!(
            "pomodoro-test-{}-{}-{}",
            name,
            std::process::id(),
            Local::now().timestamp_nanos_opt().unwrap_or_default()
        );
        path.push(unique);
        path
    }

    #[test]
    fn round_trip_notes_through_disk() {
        let root = temp_root("notes-roundtrip");
        let storage = Storage::new(&root);

        let mut notes = Notes::default();
        notes.entries.push(StudyNote {
            id: "abc".to_owned(),
            timestamp: "2026-10-17T10:00:00Z".to_owned(),
            duration_seconds: 1500,
            accomplishment: "Write tests".to_owned(),
            subject: Some("Rust".to_owned()),
            updated_at: None,
        });

        storage.save_notes(&notes).unwrap();
        let loaded = storage.load_notes().unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].id, "abc");
        assert_eq!(loaded.entries[0].accomplishment, "Write tests");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn round_trip_weekly_reflections_through_disk() {
        let root = temp_root("reflection-roundtrip");
        let storage = Storage::new(&root);
        let mut reflections = WeeklyReflections::default();
        reflections.upsert(WeeklyReflection {
            week_start: "2026-11-16".to_owned(),
            went_well: "Finished a chapter".to_owned(),
            adjust_next_time: "Take longer breaks".to_owned(),
            next_week_focus: "Practice Rust".to_owned(),
        });

        storage.save_reflections(&reflections).unwrap();
        let loaded = storage.load_reflections().unwrap();
        let saved = loaded.for_week("2026-11-16");
        assert_eq!(saved.went_well, "Finished a chapter");
        assert_eq!(saved.next_week_focus, "Practice Rust");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn weekly_reflection_upsert_keeps_one_entry_per_week() {
        let mut reflections = WeeklyReflections::default();
        reflections.upsert(WeeklyReflection {
            week_start: "2026-11-16".to_owned(),
            went_well: "First".to_owned(),
            ..WeeklyReflection::default()
        });
        reflections.upsert(WeeklyReflection {
            week_start: "2026-11-16".to_owned(),
            went_well: "Updated".to_owned(),
            ..WeeklyReflection::default()
        });

        assert_eq!(reflections.entries.len(), 1);
        assert_eq!(reflections.entries[0].went_well, "Updated");
    }

    #[test]
    fn edit_updates_note_on_disk() {
        let root = temp_root("notes-edit");
        let storage = Storage::new(&root);

        let mut notes = Notes::default();
        notes.entries.push(StudyNote {
            id: "abc".to_owned(),
            timestamp: "2026-10-17T10:00:00Z".to_owned(),
            duration_seconds: 1500,
            accomplishment: "Original".to_owned(),
            subject: None,
            updated_at: None,
        });
        storage.save_notes(&notes).unwrap();

        notes.entries[0].accomplishment = "Edited".to_owned();
        notes.entries[0].updated_at = Some("2026-10-17T11:00:00Z".to_owned());
        storage.save_notes(&notes).unwrap();

        let loaded = storage.load_notes().unwrap();
        assert_eq!(loaded.entries[0].accomplishment, "Edited");
        assert_eq!(loaded.entries[0].updated_at.as_deref(), Some("2026-10-17T11:00:00Z"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn check_writable_creates_directory_and_returns_ok() {
        let root = temp_root("writable");
        let storage = Storage::new(&root);
        storage.check_writable().unwrap();
        assert!(root.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn save_leaves_no_temp_files_on_success() {
        let root = temp_root("no-temp");
        let storage = Storage::new(&root);
        storage.save_notes(&Notes::default()).unwrap();

        let data_dir = root.join("data");
        let leftovers: Vec<_> = fs::read_dir(&data_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files leaked: {leftovers:?}");

        let _ = fs::remove_dir_all(&root);
    }
}
