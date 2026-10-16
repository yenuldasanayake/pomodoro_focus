use chrono::{Local, NaiveDate};
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
    pub escape_exits_fullscreen: bool,
    pub block_alt_tab: bool,
    pub sites_blocked: bool,
    /// Eight-digit PIN shown on the desktop and typed into the phone page.
    /// An empty string means "no PIN chosen yet" — `AppState::new` generates
    /// one and persists it on first launch.
    pub pin: String,
    /// TCP port for the phone server. `0` means "let the OS pick a free port
    /// on startup, then persist whatever we got".
    pub port: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            study_minutes: 25,
            short_break_minutes: 5,
            long_break_minutes: 15,
            sessions_before_long_break: 4,
            escape_exits_fullscreen: false,
            block_alt_tab: false,
            sites_blocked: false,
            pin: String::new(),
            port: 0,
        }
    }
}

impl Settings {
    pub fn sanitize(&mut self) {
        self.study_minutes = self.study_minutes.clamp(1, 180);
        self.short_break_minutes = self.short_break_minutes.clamp(1, 60);
        self.long_break_minutes = self.long_break_minutes.clamp(1, 120);
        self.sessions_before_long_break = self.sessions_before_long_break.clamp(1, 12);
        // Port 0 is legal ("pick any"); anything below 1024 is privileged and
        // will usually fail for a normal user, so treat it as 0.
        if self.port < 1024 {
            self.port = 0;
        }
        // If a PIN is present, require exactly 8 numeric digits; otherwise clear
        // it so the app regenerates on next launch.
        if !self.pin.is_empty()
            && (self.pin.len() != 8 || !self.pin.chars().all(|c| c.is_ascii_digit()))
        {
            self.pin.clear();
        }
    }
}

#[cfg(test)]
mod settings_tests {
    use super::Settings;

    #[test]
    fn older_settings_without_escape_option_default_to_disabled() {
        let settings: Settings = serde_json::from_str(
            r#"{"study_minutes":25,"short_break_minutes":5,"long_break_minutes":15,"sessions_before_long_break":4}"#,
        )
        .unwrap();

        assert!(!settings.escape_exits_fullscreen);
        assert!(!settings.block_alt_tab);
        assert!(!settings.sites_blocked);
        assert!(settings.pin.is_empty());
        assert_eq!(settings.port, 0);
    }

    #[test]
    fn alt_tab_block_setting_is_persisted() {
        let settings = Settings {
            block_alt_tab: true,
            ..Settings::default()
        };

        let saved = serde_json::to_string(&settings).unwrap();
        let loaded: Settings = serde_json::from_str(&saved).unwrap();

        assert!(loaded.block_alt_tab);
    }

    #[test]
    fn sites_blocked_setting_is_persisted() {
        let settings = Settings {
            sites_blocked: true,
            ..Settings::default()
        };

        let saved = serde_json::to_string(&settings).unwrap();
        let loaded: Settings = serde_json::from_str(&saved).unwrap();

        assert!(loaded.sites_blocked);
    }

    #[test]
    fn pin_and_port_round_trip() {
        let settings = Settings {
            pin: "12345678".to_owned(),
            port: 51234,
            ..Settings::default()
        };

        let saved = serde_json::to_string(&settings).unwrap();
        let loaded: Settings = serde_json::from_str(&saved).unwrap();

        assert_eq!(loaded.pin, "12345678");
        assert_eq!(loaded.port, 51234);
    }

    #[test]
    fn sanitize_rejects_malformed_pin_and_privileged_port() {
        let mut settings = Settings {
            pin: "abc".to_owned(),
            port: 80,
            ..Settings::default()
        };
        settings.sanitize();

        assert!(settings.pin.is_empty());
        assert_eq!(settings.port, 0);
    }

    #[test]
    fn sanitize_accepts_valid_pin_and_high_port() {
        let mut settings = Settings {
            pin: "98765432".to_owned(),
            port: 51234,
            ..Settings::default()
        };
        settings.sanitize();

        assert_eq!(settings.pin, "98765432");
        assert_eq!(settings.port, 51234);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub version: u32,
    pub daily_seconds: BTreeMap<String, u64>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            daily_seconds: BTreeMap::new(),
        }
    }
}

impl History {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn today_seconds(&self) -> u64 {
        self.daily_seconds
            .get(&Local::now().date_naive().to_string())
            .copied()
            .unwrap_or_default()
    }

    pub fn add_today(&mut self, seconds: u64) {
        let day = Local::now().date_naive().to_string();
        *self.daily_seconds.entry(day).or_default() += seconds;
    }

    pub fn best_day(&self) -> Option<(&str, u64)> {
        self.daily_seconds
            .iter()
            .max_by_key(|(_, seconds)| *seconds)
            .map(|(day, seconds)| (day.as_str(), *seconds))
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

    fn has_study_on(&self, day: NaiveDate) -> bool {
        self.daily_seconds
            .get(&day.to_string())
            .is_some_and(|seconds| *seconds > 0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudyNote {
    pub timestamp: String,
    pub duration_seconds: u64,
    pub accomplishment: String,
    #[serde(default)]
    pub subject: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Notes {
    pub entries: Vec<StudyNote>,
}

#[derive(Debug, Clone)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
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
        let mut history: History =
            self.load_or_default(Path::new("data").join("history.json"))?;
        if history.version == 0 {
            history.version = History::CURRENT_VERSION;
        }
        Ok(history)
    }

    pub fn save_history(&self, history: &History) -> io::Result<()> {
        self.save(Path::new("data").join("history.json"), history)
    }

    pub fn load_notes(&self) -> io::Result<Notes> {
        self.load_or_default(Path::new("data").join("notes.json"))
    }

    pub fn save_notes(&self, notes: &Notes) -> io::Result<()> {
        self.save(Path::new("data").join("notes.json"), notes)
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
                    eprintln!(
                        "Could not parse {}: {error}; using defaults",
                        path.display()
                    );
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
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
        fs::write(path, bytes)
    }
}