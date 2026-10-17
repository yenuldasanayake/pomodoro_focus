//! Serialization for `/export/*` endpoints and the desktop "Export data…"
//! button. Everything here is pure data-in, bytes-out so it's easy to test.

use crate::storage::{History, Notes};
use serde::Serialize;

/// Version stamp written into `all.json` so future restores can tell which
/// format they're looking at.
pub const EXPORT_VERSION: u32 = 1;

#[derive(Serialize)]
pub struct ExportBundle<'a> {
    pub version: u32,
    pub exported_at: String,
    pub history: &'a History,
    pub notes: &'a Notes,
}

pub fn all_json(history: &History, notes: &Notes) -> String {
    let bundle = ExportBundle {
        version: EXPORT_VERSION,
        exported_at: chrono::Local::now().to_rfc3339(),
        history,
        notes,
    };
    // This cannot fail: all field types are serde-compatible primitives.
    serde_json::to_string_pretty(&bundle).unwrap_or_else(|_| "{}".to_owned())
}

/// CSV of `daily_seconds`, oldest date first.
/// Columns: date,seconds,minutes
pub fn history_csv(history: &History) -> String {
    let mut out = String::from("date,seconds,minutes\n");
    for (date, seconds) in &history.daily_seconds {
        out.push_str(date);
        out.push(',');
        out.push_str(&seconds.to_string());
        out.push(',');
        out.push_str(&format!("{:.2}", *seconds as f64 / 60.0));
        out.push('\n');
    }
    out
}

/// CSV of `notes.entries`, oldest first.
/// Columns: id,timestamp,duration_seconds,subject,accomplishment,updated_at
pub fn notes_csv(notes: &Notes) -> String {
    let mut out =
        String::from("id,timestamp,duration_seconds,subject,accomplishment,updated_at\n");
    for note in &notes.entries {
        out.push_str(&csv_field(&note.id));
        out.push(',');
        out.push_str(&csv_field(&note.timestamp));
        out.push(',');
        out.push_str(&note.duration_seconds.to_string());
        out.push(',');
        out.push_str(&csv_field(note.subject.as_deref().unwrap_or("")));
        out.push(',');
        out.push_str(&csv_field(&note.accomplishment));
        out.push(',');
        out.push_str(&csv_field(note.updated_at.as_deref().unwrap_or("")));
        out.push('\n');
    }
    out
}

/// Combined CSV: one row per note, with the day's total attached.
/// Columns: date,today_seconds,id,timestamp,duration_seconds,subject,accomplishment
pub fn all_csv(history: &History, notes: &Notes) -> String {
    let mut out = String::from(
        "date,today_seconds,id,timestamp,duration_seconds,subject,accomplishment\n",
    );
    for note in &notes.entries {
        let day = note
            .timestamp
            .get(..10)
            .unwrap_or("")
            .to_owned();
        let day_total = history
            .daily_seconds
            .get(&day)
            .copied()
            .unwrap_or_default();
        out.push_str(&csv_field(&day));
        out.push(',');
        out.push_str(&day_total.to_string());
        out.push(',');
        out.push_str(&csv_field(&note.id));
        out.push(',');
        out.push_str(&csv_field(&note.timestamp));
        out.push(',');
        out.push_str(&note.duration_seconds.to_string());
        out.push(',');
        out.push_str(&csv_field(note.subject.as_deref().unwrap_or("")));
        out.push(',');
        out.push_str(&csv_field(&note.accomplishment));
        out.push('\n');
    }
    out
}

/// Quote a CSV field if it contains a comma, quote, or newline. Doubles any
/// embedded quotes per RFC 4180.
fn csv_field(value: &str) -> String {
    let needs_quotes = value
        .chars()
        .any(|c| matches!(c, ',' | '"' | '\n' | '\r'));
    if !needs_quotes {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        if c == '"' {
            out.push('"');
            out.push('"');
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

/// Write all four export files into a timestamped folder under `root`.
/// Returns the path to that folder.
pub fn write_to_disk(
    root: &std::path::Path,
    history: &History,
    notes: &Notes,
) -> std::io::Result<std::path::PathBuf> {
    let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S").to_string();
    let dir = root.join("exports").join(&stamp);
    std::fs::create_dir_all(&dir)?;

    std::fs::write(dir.join("history.csv"), history_csv(history))?;
    std::fs::write(dir.join("notes.csv"), notes_csv(notes))?;
    std::fs::write(dir.join("all.csv"), all_csv(history, notes))?;
    std::fs::write(dir.join("all.json"), all_json(history, notes))?;

    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{History, Notes, StudyNote};
    use std::collections::BTreeMap;

    fn sample_history() -> History {
        let mut history = History::default();
        let mut daily = BTreeMap::new();
        daily.insert("2026-10-01".to_owned(), 1500);
        daily.insert("2026-10-02".to_owned(), 900);
        history.daily_seconds = daily;
        history
    }

    fn sample_notes() -> Notes {
        Notes {
            entries: vec![StudyNote {
                id: "abc123".to_owned(),
                timestamp: "2026-10-01T10:00:00+05:30".to_owned(),
                duration_seconds: 1500,
                accomplishment: "Wrote tests, fixed bugs".to_owned(),
                subject: Some("Rust".to_owned()),
                updated_at: None,
            }],
        }
    }

    #[test]
    fn history_csv_has_header_and_rows() {
        let csv = history_csv(&sample_history());
        assert!(csv.starts_with("date,seconds,minutes\n"));
        assert!(csv.contains("2026-10-01,1500,25.00"));
        assert!(csv.contains("2026-10-02,900,15.00"));
    }

    #[test]
    fn notes_csv_quotes_fields_with_commas() {
        let csv = notes_csv(&sample_notes());
        // The accomplishment contains a comma, so it must be quoted.
        assert!(csv.contains("\"Wrote tests, fixed bugs\""));
        assert!(csv.contains("Rust"));
        assert!(csv.contains("abc123"));
    }

    #[test]
    fn all_json_round_trips() {
        let json = all_json(&sample_history(), &sample_notes());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["version"], EXPORT_VERSION);
        assert_eq!(
            parsed["history"]["daily_seconds"]["2026-10-01"],
            1500
        );
        assert_eq!(
            parsed["notes"]["entries"][0]["accomplishment"],
            "Wrote tests, fixed bugs"
        );
    }

    #[test]
    fn csv_field_escapes_quotes() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("has,comma"), "\"has,comma\"");
        assert_eq!(csv_field("has \"quote\""), "\"has \"\"quote\"\"\"");
        assert_eq!(csv_field("has\nnewline"), "\"has\nnewline\"");
    }

    #[test]
    fn write_to_disk_creates_all_files() {
        let root = std::env::temp_dir().join(format!(
            "pomodoro-export-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();

        let dir = write_to_disk(&root, &sample_history(), &sample_notes()).unwrap();
        assert!(dir.join("history.csv").exists());
        assert!(dir.join("notes.csv").exists());
        assert!(dir.join("all.csv").exists());
        assert!(dir.join("all.json").exists());

        let contents = std::fs::read_to_string(dir.join("all.json")).unwrap();
        assert!(contents.contains("\"version\": 1"));

        let _ = std::fs::remove_dir_all(&root);
    }
}