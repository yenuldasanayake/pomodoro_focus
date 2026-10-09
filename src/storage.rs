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
        }
    }
}

impl Settings {
    pub fn sanitize(&mut self) {
        self.study_minutes = self.study_minutes.clamp(1, 180);
        self.short_break_minutes = self.short_break_minutes.clamp(1, 60);
        self.long_break_minutes = self.long_break_minutes.clamp(1, 120);
        self.sessions_before_long_break = self.sessions_before_long_break.clamp(1, 12);
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub daily_seconds: BTreeMap<String, u64>,
}

impl History {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BlockingSettings {
    pub sites_blocked: bool,
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
        self.load_or_default(Path::new("data").join("history.json"))
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

    pub fn load_blocking_settings(&self) -> io::Result<BlockingSettings> {
        self.load_or_default(Path::new("data").join("blocking.json"))
    }

    pub fn save_blocking_settings(&self, settings: &BlockingSettings) -> io::Result<()> {
        self.save(Path::new("data").join("blocking.json"), settings)
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
