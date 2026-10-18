//! Import a previously exported `all.json`. Two modes:
//!
//! - `Replace`: wipe existing history and notes, insert the imported data.
//! - `Merge`: keep existing, add missing data.
//!
//! The merge is intentionally conservative: for history we take the
//! maximum per day (never sum, which would double-count); for notes we
//! skip ids that already exist.

use crate::storage::{History, Notes};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Bundle {
    pub version: u32,
    #[serde(default)]
    pub exported_at: String,
    pub history: History,
    pub notes: Notes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Replace,
    Merge,
}

#[derive(Debug, Clone)]
pub struct Preview {
    pub version: u32,
    pub exported_at: String,
    pub history_days: usize,
    pub history_seconds: u64,
    pub notes: usize,
    pub overlapping_days: Vec<String>,
    pub overlapping_note_ids: Vec<String>,
}

pub fn parse(bytes: &[u8]) -> Result<Bundle, String> {
    let bundle: Bundle =
        serde_json::from_slice(bytes).map_err(|error| format!("Could not parse file: {error}"))?;
    if bundle.version == 0 {
        return Err("File has no version — was it exported from this app?".to_owned());
    }
    if bundle.version > crate::export::EXPORT_VERSION {
        return Err(format!(
            "File version {} is newer than this app supports ({}). Update Pomodoro Focus.",
            bundle.version,
            crate::export::EXPORT_VERSION,
        ));
    }
    Ok(bundle)
}

pub fn preview(bundle: &Bundle, current_history: &History, current_notes: &Notes) -> Preview {
    let history_seconds: u64 = bundle.history.daily_seconds.values().sum();
    let overlapping_days: Vec<String> = bundle
        .history
        .daily_seconds
        .keys()
        .filter(|day| current_history.daily_seconds.contains_key(*day))
        .cloned()
        .collect();
    let overlapping_note_ids: Vec<String> = bundle
        .notes
        .entries
        .iter()
        .filter(|note| !note.id.is_empty() && current_notes.find_index(&note.id).is_some())
        .map(|note| note.id.clone())
        .collect();

    Preview {
        version: bundle.version,
        exported_at: bundle.exported_at.clone(),
        history_days: bundle.history.daily_seconds.len(),
        history_seconds,
        notes: bundle.notes.entries.len(),
        overlapping_days,
        overlapping_note_ids,
    }
}

pub fn apply(
    bundle: Bundle,
    current_history: &mut History,
    current_notes: &mut Notes,
    mode: Mode,
) -> Report {
    match mode {
        Mode::Replace => apply_replace(bundle, current_history, current_notes),
        Mode::Merge => apply_merge(bundle, current_history, current_notes),
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub history_days_added: usize,
    pub history_days_updated: usize,
    pub notes_added: usize,
    pub notes_skipped: usize,
}

fn apply_replace(bundle: Bundle, history: &mut History, notes: &mut Notes) -> Report {
    let report = Report {
        history_days_added: bundle.history.daily_seconds.len(),
        history_days_updated: 0,
        notes_added: bundle.notes.entries.len(),
        notes_skipped: 0,
    };
    *history = bundle.history;
    *notes = bundle.notes;
    notes.ensure_ids();
    report
}

fn apply_merge(bundle: Bundle, history: &mut History, notes: &mut Notes) -> Report {
    let mut report = Report::default();

    for (day, seconds) in bundle.history.daily_seconds {
        match history.daily_seconds.get_mut(&day) {
            Some(existing) => {
                if *existing < seconds {
                    *existing = seconds;
                    report.history_days_updated += 1;
                }
            }
            None => {
                history.daily_seconds.insert(day, seconds);
                report.history_days_added += 1;
            }
        }
    }

    let mut existing_ids: std::collections::HashSet<String> =
        notes.entries.iter().map(|note| note.id.clone()).collect();

    let mut imported = bundle.notes;
    imported.ensure_ids();

    for note in imported.entries {
        if existing_ids.contains(&note.id) {
            report.notes_skipped += 1;
        } else {
            existing_ids.insert(note.id.clone());
            notes.entries.push(note);
            report.notes_added += 1;
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StudyNote;

    fn note(id: &str, accomplishment: &str) -> StudyNote {
        StudyNote {
            id: id.to_owned(),
            timestamp: "2026-10-01T10:00:00Z".to_owned(),
            duration_seconds: 1500,
            accomplishment: accomplishment.to_owned(),
            subject: None,
            updated_at: None,
        }
    }

    fn bundle(days: &[(&str, u64)], notes: Vec<StudyNote>) -> Bundle {
        let mut history = History::default();
        for (day, seconds) in days {
            history.daily_seconds.insert((*day).to_owned(), *seconds);
        }
        Bundle {
            version: 1,
            exported_at: "2026-10-01T00:00:00Z".to_owned(),
            history,
            notes: Notes { entries: notes },
        }
    }

    #[test]
    fn parse_rejects_missing_version() {
        let json = br#"{"version":0,"history":{"version":1,"daily_seconds":{}},"notes":{"entries":[]}}"#;
        assert!(parse(json).is_err());
    }

    #[test]
    fn parse_rejects_future_version() {
        let json = br#"{"version":99,"history":{"version":1,"daily_seconds":{}},"notes":{"entries":[]}}"#;
        assert!(parse(json).is_err());
    }

    #[test]
    fn merge_keeps_higher_daily_seconds() {
        let mut history = History::default();
        history.daily_seconds.insert("2026-10-01".to_owned(), 500);
        let mut notes = Notes::default();

        let report = apply(
            bundle(&[("2026-10-01", 1500)], vec![]),
            &mut history,
            &mut notes,
            Mode::Merge,
        );

        assert_eq!(*history.daily_seconds.get("2026-10-01").unwrap(), 1500);
        assert_eq!(report.history_days_updated, 1);
        assert_eq!(report.history_days_added, 0);
    }

    #[test]
    fn merge_keeps_higher_existing_when_import_is_lower() {
        let mut history = History::default();
        history.daily_seconds.insert("2026-10-01".to_owned(), 2000);
        let mut notes = Notes::default();

        apply(
            bundle(&[("2026-10-01", 1000)], vec![]),
            &mut history,
            &mut notes,
            Mode::Merge,
        );

        assert_eq!(*history.daily_seconds.get("2026-10-01").unwrap(), 2000);
    }

    #[test]
    fn merge_adds_non_overlapping_days() {
        let mut history = History::default();
        history.daily_seconds.insert("2026-10-01".to_owned(), 500);
        let mut notes = Notes::default();

        let report = apply(
            bundle(&[("2026-10-02", 1000)], vec![]),
            &mut history,
            &mut notes,
            Mode::Merge,
        );

        assert!(history.daily_seconds.contains_key("2026-10-02"));
        assert_eq!(report.history_days_added, 1);
    }

    #[test]
    fn merge_skips_existing_notes_and_adds_new() {
        let mut history = History::default();
        let mut notes = Notes {
            entries: vec![note("a", "old")],
        };

        let report = apply(
            bundle(&[], vec![note("a", "dup"), note("b", "new")]),
            &mut history,
            &mut notes,
            Mode::Merge,
        );

        assert_eq!(report.notes_added, 1);
        assert_eq!(report.notes_skipped, 1);
        assert_eq!(notes.entries.len(), 2);
        assert!(notes.find_index("a").is_some());
        assert!(notes.find_index("b").is_some());
    }

    #[test]
    fn replace_wipes_and_replaces() {
        let mut history = History::default();
        history.daily_seconds.insert("2026-09-01".to_owned(), 999);
        let mut notes = Notes {
            entries: vec![note("old", "gone")],
        };

        apply(
            bundle(&[("2026-10-01", 1000)], vec![note("new", "in")]),
            &mut history,
            &mut notes,
            Mode::Replace,
        );

        assert!(!history.daily_seconds.contains_key("2026-09-01"));
        assert_eq!(*history.daily_seconds.get("2026-10-01").unwrap(), 1000);
        assert_eq!(notes.entries.len(), 1);
        assert_eq!(notes.entries[0].id, "new");
    }

    #[test]
    fn replace_assigns_ids_to_legacy_notes() {
        let mut history = History::default();
        let mut notes = Notes::default();
        let mut legacy = note("", "no id");
        legacy.id.clear();

        apply(
            bundle(&[], vec![legacy]),
            &mut history,
            &mut notes,
            Mode::Replace,
        );

        assert_eq!(notes.entries.len(), 1);
        assert!(!notes.entries[0].id.is_empty());
    }
}