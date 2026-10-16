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
    pub pin: String,
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
        if self.port < 1024 {
            self.port = 0;
        }
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
            entries: vec![
                StudyNote {
                    id: String::new(),
                    ..sample_note("")
                },
                sample_note("kept"),
            ],
        };

        assert!(notes.ensure_ids());
        assert!(!notes.entries[0].id.is_empty());
        assert_eq!(notes.entries[1].id, "kept");
        assert!(!notes.ensure_ids(), "second pass should be a no-op");
    }

    #[test]
    fn find_index_locates_by_id() {
        let notes = Notes {
            entries: vec![sample_note("a"), sample_note("b")],
        };
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

    /// Probe that the root directory is writable. Used at startup to warn the
    /// user early instead of failing silently on the first note edit.
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

    /// Diagnostic helper. Reports whether a specific data file can be written,
    /// and if not, why. Called when a save fails so we can produce a useful
    /// error message instead of a bare `os error 5`.
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

    /// Atomically write a JSON file. Writes to a sibling temp file first, then
    /// renames over the target. This survives crashes mid-write and produces a
    /// useful error if the destination is read-only or locked.
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

        // Temp file lives next to the target so the rename is atomic on the
        // same filesystem.
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

        // Sync before rename so a crash right after this point still leaves
        // either the old or the new file fully intact.
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
        assert_eq!(
            loaded.entries[0].updated_at.as_deref(),
            Some("2026-10-17T11:00:00Z")
        );

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
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("tmp")
            })
            .collect();
        assert!(leftovers.is_empty(), "temp files leaked: {leftovers:?}");

        let _ = fs::remove_dir_all(&root);
    }
}