use crate::storage::{History, Notes};
use chrono::{Datelike, Duration, Local};
use eframe::egui::{self, Color32};
use egui_plot::{Bar, BarChart, Plot};
use std::collections::BTreeMap;

pub fn show(ui: &mut egui::Ui, history: &History, notes: &Notes) {
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.heading("Your rhythm");
        ui.label(
            egui::RichText::new(format!("🔥 {} day streak", history.current_streak()))
                .color(Color32::from_rgb(250, 179, 135)),
        );
        if let Some((day, seconds)) = history.best_day() {
            ui.label(
                egui::RichText::new(format!("Best · {day} · {}", hours_and_minutes(seconds)))
                    .color(crate::theme::MUTED),
            );
        }
    });

    ui.columns(2, |columns| {
        columns[0].heading("Last 14 days");
        Plot::new("daily-study")
            .height(180.0)
            .show_axes([false, true])
            .show(&mut columns[0], |plot_ui| {
                let today = Local::now().date_naive();
                let bars = (0..14)
                    .rev()
                    .map(|index| {
                        let day = today - Duration::days(index);
                        let seconds = history
                            .daily_seconds
                            .get(&day.to_string())
                            .copied()
                            .unwrap_or_default();
                        Bar::new((13 - index) as f64, seconds as f64 / 3600.0)
                            .name(day.format("%a %d").to_string())
                            .width(0.68)
                            .fill(Color32::from_rgb(235, 118, 128))
                    })
                    .collect();
                plot_ui.bar_chart(BarChart::new("Study hours", bars));
            });

        columns[1].heading("Last 8 weeks");
        Plot::new("weekly-study")
            .height(180.0)
            .show_axes([false, true])
            .show(&mut columns[1], |plot_ui| {
                let today = Local::now().date_naive();
                let monday =
                    today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
                let bars = (0..8)
                    .rev()
                    .enumerate()
                    .map(|(index, week_offset)| {
                        let start = monday - Duration::days(i64::from(week_offset) * 7);
                        let seconds = (0..7)
                            .map(|day_offset| {
                                let day = start + Duration::days(day_offset);
                                history
                                    .daily_seconds
                                    .get(&day.to_string())
                                    .copied()
                                    .unwrap_or_default()
                            })
                            .sum::<u64>();
                        let iso = start.iso_week();
                        Bar::new(index as f64, seconds as f64 / 3600.0)
                            .name(format!("{} W{:02}", iso.year(), iso.week()))
                            .width(0.68)
                            .fill(Color32::from_rgb(137, 180, 250))
                    })
                    .collect();
                plot_ui.bar_chart(BarChart::new("Study hours", bars));
            });
    });

    ui.add_space(8.0);
    ui.heading("Subjects");
    let mut subjects: BTreeMap<String, u64> = BTreeMap::new();
    for note in &notes.entries {
        let name = note
            .subject
            .as_deref()
            .filter(|subject| !subject.trim().is_empty())
            .unwrap_or("Untagged");
        *subjects.entry(name.to_owned()).or_default() += note.duration_seconds;
    }
    if subjects.is_empty() {
        ui.label(
            egui::RichText::new("Complete a focus session to see your subjects here.")
                .color(crate::theme::MUTED),
        );
    } else {
        let bars = subjects
            .into_iter()
            .enumerate()
            .map(|(index, (subject, seconds))| {
                Bar::new(index as f64, seconds as f64 / 3600.0)
                    .name(subject)
                    .width(0.68)
                    .fill(Color32::from_rgb(166, 227, 161))
            })
            .collect();
        Plot::new("subject-breakdown")
            .height(170.0)
            .show(&mut *ui, |plot_ui| {
                plot_ui.bar_chart(BarChart::new("Study hours", bars));
            });
    }
}

fn hours_and_minutes(seconds: u64) -> String {
    format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
}
