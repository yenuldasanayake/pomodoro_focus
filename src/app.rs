use crate::{
    alt_tab, breaks, charts, export, import, site_blocker, tray,
    server::{self, AppState, PowerAction, SharedState},
    storage::Storage,
    theme,
    timer::Mode,
};
use eframe::egui::{self, Align2, Color32, FontId, Id, Key, Pos2, Sense, Stroke, Vec2};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub struct PomodoroApp {
    shared: SharedState,
    note_text: String,
    subject_text: String,
    previous_mode: Mode,
    transition_from: Color32,
    transition_started: Instant,
    editing_note: Option<EditingNote>,
    undo_started: Option<Instant>,
    tray: Option<tray::TrayHandle>,
    quit_from_tray: bool,
    import_path: String,
    show_import_dialog: bool,
    import_mode: import::Mode,
    app_is_fullscreen: bool,
}

#[derive(Clone)]
struct EditingNote {
    id: String,
    accomplishment: String,
    subject: String,
}

impl PomodoroApp {
    pub fn new(ctx: &eframe::CreationContext<'_>) -> Result<Self, getrandom::Error> {
        let storage = Storage::new(".");
        let state = AppState::new(ctx.egui_ctx.clone(), storage)?;
        let previous_mode = state.timer.mode;
        let transition_from = theme::accent(previous_mode);
        let shared = Arc::new(Mutex::new(state));
        server::start_server(shared.clone());

        let tray = match tray::TrayHandle::new() {
            Ok(handle) => Some(handle),
            Err(error) => {
                eprintln!("Could not create tray icon: {error}");
                None
            }
        };
        if tray.is_none() {
            ctx.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        }

        Ok(Self {
            shared,
            note_text: String::new(),
            subject_text: String::new(),
            previous_mode,
            transition_from,
            transition_started: Instant::now(),
            editing_note: None,
            undo_started: None,
            tray,
            quit_from_tray: false,
            import_path: String::new(),
            show_import_dialog: false,
            import_mode: import::Mode::Merge,
            app_is_fullscreen: false,
        })
    }
}

impl eframe::App for PomodoroApp {
    fn ui(&mut self, root_ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root_ui.ctx().clone();
        theme::apply(&ctx);

        if let Some(tray) = &self.tray {
            for event in tray.poll() {
                match event {
                    tray::TrayEvent::ShowWindow => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    tray::TrayEvent::HideWindow => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    }
                    tray::TrayEvent::ToggleTimer => {
                        if let Ok(mut state) = self.shared.lock() {
                            let command = if state.timer.running { "pause" } else { "start" };
                            let _ = state.execute_command(command);
                        }
                    }
                    tray::TrayEvent::Quit => {
                        self.quit_from_tray = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }

        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_from_tray {
            let mut minimize_instead = false;
            if let Ok(state) = self.shared.lock() {
                if state.settings.close_to_tray || state.timer.running {
                    minimize_instead = true;
                }
            }
            if minimize_instead {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }

        let shared = self.shared.clone();
        let Ok(mut state) = shared.lock() else {
            egui::CentralPanel::default().show(root_ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label("The timer state is unavailable. Please restart the app.");
                });
            });
            return;
        };

        if state.should_restore_window {
            state.should_restore_window = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.request_repaint();
        }

        {
            let minutes = state.timer.remaining_seconds / 60;
            let seconds = state.timer.remaining_seconds % 60;
            let label = match state.timer.mode {
                Mode::Study => "FOCUS",
                Mode::ShortBreak => "SHORT BREAK",
                Mode::LongBreak => "LONG BREAK",
            };
            let title = format!("Pomodoro Focus — {minutes:02}:{seconds:02} · {label}");
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }

        // Escape toggles fullscreen. We track the state in a local bool so
        // repeated toggles are deterministic even if the OS viewport reports
        // stale fullscreen information mid-transition.
        if state.settings.escape_exits_fullscreen
            && ctx.input(|input| input.key_pressed(Key::Escape))
        {
            self.app_is_fullscreen = !self.app_is_fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.app_is_fullscreen));
        }
        // Resync our local flag from the OS when it reports a definitive value.
        if let Some(v) = ctx.input(|input| input.viewport().fullscreen) {
            self.app_is_fullscreen = v;
        }

        let text_editing = ctx.memory(|memory| memory.focused().is_some());
        if !text_editing {
            if ctx.input(|input| input.key_pressed(Key::Space)) {
                let command = if state.timer.running { "pause" } else { "start" };
                if let Err(error) = state.execute_command(command) {
                    state.error_message = Some(error);
                }
            }
            let plain = |m: &egui::Modifiers| !m.ctrl && !m.command && !m.alt && !m.shift;
            if ctx.input(|input| input.key_pressed(Key::R) && plain(&input.modifiers))
                && let Err(error) = state.execute_command("reset")
            {
                state.error_message = Some(error);
            }
            if ctx.input(|input| input.key_pressed(Key::S) && plain(&input.modifiers))
                && let Err(error) = state.execute_command("skip")
            {
                state.error_message = Some(error);
            }
        }

        state.tick();
        if state.timer.mode != self.previous_mode {
            self.transition_from = theme::accent(self.previous_mode);
            self.previous_mode = state.timer.mode;
            self.transition_started = Instant::now();
        }
        let target = theme::accent(state.timer.mode);
        let blend = (self.transition_started.elapsed().as_secs_f32() / 0.55).clamp(0.0, 1.0);
        let accent = mix_color(self.transition_from, target, blend);
        if blend < 1.0 {
            ctx.request_repaint_after(Duration::from_millis(24));
        }
        if state.timer.running {
            ctx.request_repaint_after(Duration::from_millis(160));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BACKGROUND))
            .show(root_ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(28, 24))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                self.show_top(ui, &state);
                                ui.add_space(14.0);
                                ui.vertical_centered(|ui| {
                                    self.show_timer(ui, &state, accent);
                                    ui.add_space(12.0);
                                    self.show_controls(ui, &mut state, accent);
                                    ui.add_space(8.0);
                                    ui.label(
                                        egui::RichText::new(
                                            "Space · start/pause   ·   R · reset   ·   S · skip",
                                        )
                                        .size(11.0)
                                        .color(theme::MUTED),
                                    );
                                });
                                ui.add_space(24.0);
                                self.show_connection(ui, &mut state);
                                ui.add_space(8.0);
                                self.show_settings(ui, &mut state);
                                charts::show(ui, &state.history, &state.notes);
                                ui.add_space(14.0);
                                self.show_notes_section(ui, &mut state);
                            });
                    });
            });

        self.show_note_popup(&ctx, &mut state);
        self.show_undo_toast(&ctx, &mut state);
        self.show_power_banner(&ctx, &mut state);
        self.show_import_dialog(&ctx, &mut state);

        if let Some(message) = &state.server_error {
            egui::Area::new(Id::new("server-error"))
                .anchor(Align2::RIGHT_TOP, Vec2::new(-24.0, 24.0))
                .show(&ctx, |ui| {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(69, 35, 49))
                        .corner_radius(16.0)
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(message).color(theme::TEXT));
                        });
                });
        }
        if let Some(message) = &state.error_message {
            egui::Area::new(Id::new("storage-error"))
                .anchor(Align2::LEFT_BOTTOM, Vec2::new(24.0, -20.0))
                .show(&ctx, |ui| {
                    ui.label(
                        egui::RichText::new(message).color(Color32::from_rgb(243, 139, 168)),
                    );
                });
        }
    }

    fn on_exit(&mut self) {
        if let Ok(mut state) = self.shared.lock() {
            state.flush_all();
            let _ = alt_tab::set_blocked(false);
            let _ = site_blocker::set_blocked(false);
        }
    }
}

impl PomodoroApp {
    fn show_top(&self, ui: &mut egui::Ui, state: &AppState) {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("POMODORO FOCUS")
                    .size(15.0)
                    .strong()
                    .color(theme::TEXT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{:02}  COMPLETED",
                        state.timer.completed_study_sessions
                    ))
                    .size(12.0)
                    .strong()
                    .color(theme::MUTED),
                );
                ui.add_space(4.0);
            });
        });
        ui.add_space(12.0);
        ui.separator();
    }

    fn show_timer(&self, ui: &mut egui::Ui, state: &AppState, accent: Color32) {
        let diameter = 414.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::hover());
        let painter = ui.painter_at(rect);
        let center = rect.center();
        let radius = diameter * 0.43;
        painter.circle_stroke(
            center,
            radius,
            Stroke::new(6.0, Color32::from_rgb(60, 64, 67)),
        );

        let progress = state.timer.progress(&state.settings);
        let steps = ((progress * 200.0).ceil() as usize).max(1);
        if progress > 0.0 {
            let mut previous = ring_point(center, radius, -std::f32::consts::FRAC_PI_2);
            for step in 1..=steps {
                let angle = -std::f32::consts::FRAC_PI_2
                    + std::f32::consts::TAU * progress * step as f32 / steps as f32;
                let point = ring_point(center, radius, angle);
                painter.line_segment([previous, point], Stroke::new(6.0, accent));
                previous = point;
            }
        }

        painter.text(
            center + Vec2::new(0.0, -70.0),
            Align2::CENTER_CENTER,
            state.timer.mode.label(),
            FontId::proportional(14.0),
            accent,
        );
        let minutes = state.timer.remaining_seconds / 60;
        let seconds = state.timer.remaining_seconds % 60;
        painter.text(
            center + Vec2::new(0.0, 7.0),
            Align2::CENTER_CENTER,
            format!("{minutes:02}:{seconds:02}"),
            FontId::proportional(96.0),
            theme::TEXT,
        );

        if let Some(suggestion) =
            breaks::current_suggestion(state.timer.mode, state.timer.elapsed_in_mode)
        {
            painter.text(
                center + Vec2::new(0.0, 78.0),
                Align2::CENTER_CENTER,
                format!("💡 {suggestion}"),
                FontId::proportional(13.0),
                theme::MUTED,
            );
        } else {
            painter.text(
                center + Vec2::new(0.0, 78.0),
                Align2::CENTER_CENTER,
                format!(
                    "TODAY  {}",
                    readable_duration(state.history.today_seconds())
                ),
                FontId::proportional(14.0),
                theme::MUTED,
            );
        }

        if state.settings.daily_goal_minutes > 0 {
            let goal_secs = u64::from(state.settings.daily_goal_minutes) * 60;
            let today = state.history.today_seconds();
            let progress = (today as f32 / goal_secs as f32).clamp(0.0, 1.0);
            let bar_width = radius * 1.4;
            let bar_height = 6.0;
            let bar_rect = egui::Rect::from_center_size(
                center + Vec2::new(0.0, 148.0),
                Vec2::new(bar_width, bar_height),
            );
            painter.rect_filled(bar_rect, 3.0, Color32::from_rgb(60, 64, 67));
            let filled = egui::Rect::from_min_size(
                bar_rect.min,
                Vec2::new(bar_width * progress, bar_height),
            );
            let fill_color = if progress >= 1.0 {
                Color32::from_rgb(129, 201, 149)
            } else {
                accent
            };
            painter.rect_filled(filled, 3.0, fill_color);
            painter.text(
                center + Vec2::new(0.0, 162.0),
                Align2::CENTER_CENTER,
                format!(
                    "GOAL  {} / {}",
                    readable_duration(today),
                    readable_duration(goal_secs),
                ),
                FontId::proportional(11.0),
                theme::MUTED,
            );
        }

        painter.circle_filled(center + Vec2::new(0.0, 116.0), 3.5, accent);
    }

    fn show_controls(&self, ui: &mut egui::Ui, state: &mut AppState, accent: Color32) {
        ui.horizontal(|ui| {
            if action_button(ui, "Skip", Color32::from_rgb(60, 64, 67), theme::TEXT, 76.0)
                .clicked()
                && let Err(error) = state.execute_command("skip")
            {
                state.error_message = Some(error);
            }
            ui.add_space(12.0);
            let label = if state.timer.running { "Pause" } else { "Start focus" };
            if action_button(ui, label, accent, Color32::from_rgb(32, 33, 36), 176.0).clicked() {
                let command = if state.timer.running { "pause" } else { "start" };
                if let Err(error) = state.execute_command(command) {
                    state.error_message = Some(error);
                }
            }
            ui.add_space(12.0);
            if action_button(ui, "Reset", Color32::from_rgb(60, 64, 67), theme::TEXT, 76.0)
                .clicked()
                && let Err(error) = state.execute_command("reset")
            {
                state.error_message = Some(error);
            }
        });
    }

    fn show_connection(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        ui.add_space(12.0);
        egui::Frame::new()
            .fill(theme::PANEL)
            .corner_radius(16.0)
            .inner_margin(egui::Margin::symmetric(20, 14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("PHONE REMOTE")
                                .size(11.0)
                                .strong()
                                .color(theme::MUTED),
                        );
                        ui.horizontal(|ui| {
                            let url = if state.local_url.is_empty() {
                                "starting…".to_owned()
                            } else {
                                state.local_url.clone()
                            };
                            ui.label(egui::RichText::new(&url).color(theme::TEXT));
                            ui.label(
                                egui::RichText::new(format!(" · PIN  {}", state.pin))
                                    .strong()
                                    .color(theme::TEXT),
                            );
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Copy URL").clicked() && !state.local_url.is_empty() {
                            ui.ctx().copy_text(state.local_url.clone());
                        }
                    });
                });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("New PIN").clicked() {
                        match state.rotate_pin() {
                            Ok(()) => state.error_message = None,
                            Err(error) => state.error_message = Some(error),
                        }
                    }
                    if ui.button("New port").clicked() {
                        match state.request_new_port() {
                            Ok(()) => state.error_message = None,
                            Err(error) => state.error_message = Some(error),
                        }
                    }
                    if ui.button("Auto port").clicked() {
                        match state.request_auto_port() {
                            Ok(()) => state.error_message = None,
                            Err(error) => state.error_message = Some(error),
                        }
                    }
                    if ui.button("Export data…").clicked() {
                        let root = std::path::Path::new(".");
                        match export::write_to_disk(root, &state.history, &state.notes) {
                            Ok(path) => {
                                let display = path.display().to_string();
                                ui.ctx().copy_text(display.clone());
                                state.error_message = Some(format!(
                                    "Exported to {display} (path copied to clipboard)"
                                ));
                            }
                            Err(error) => {
                                state.error_message =
                                    Some(format!("Export failed: {error}"));
                            }
                        }
                    }
                    if ui.button("Import data…").clicked() {
                        self.import_path.clear();
                        self.show_import_dialog = true;
                    }
                });

                if state.port_change_pending {
                    ui.label(
                        egui::RichText::new("Restart the app to apply the new port.")
                            .size(12.0)
                            .color(Color32::from_rgb(250, 179, 135)),
                    );
                }

                ui.label(
                    egui::RichText::new(
                        "Open this address on a phone connected to the same Wi-Fi.",
                    )
                    .size(12.0)
                    .color(theme::MUTED),
                );
                ui.label(
                    egui::RichText::new(if state.hosts_block_active {
                        "System-wide site block is active. The browser extension follows this switch."
                    } else if state.sites_blocked {
                        "Browser extension blocking is on; run this app as Administrator to block across Windows."
                    } else {
                        "Manage website blocking from the phone page."
                    })
                    .size(12.0)
                    .color(theme::MUTED),
                );
            });
    }

    fn show_settings(&self, ui: &mut egui::Ui, state: &mut AppState) {
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.heading("Session settings");
            ui.label(
                egui::RichText::new("Changes save automatically · tray & fullscreen options on the phone")
                    .size(12.0)
                    .color(theme::MUTED),
            );
        });
        let mut settings = state.settings.clone();
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            changed |= setting_control(ui, "Focus · min", &mut settings.study_minutes, 1..=180);
            changed |= setting_control(
                ui,
                "Short break · min",
                &mut settings.short_break_minutes,
                1..=60,
            );
            changed |= setting_control(
                ui,
                "Long break · min",
                &mut settings.long_break_minutes,
                1..=120,
            );
            changed |= setting_control(
                ui,
                "Sessions per cycle",
                &mut settings.sessions_before_long_break,
                1..=12,
            );
            changed |= setting_control(
                ui,
                "Daily goal · min (0 = off)",
                &mut settings.daily_goal_minutes,
                0..=720,
            );
        });
        ui.add_space(4.0);
        if ui
            .checkbox(&mut settings.auto_start_next, "Auto-start next session")
            .changed()
        {
            changed = true;
        }
        if ui
            .checkbox(
                &mut settings.remote_power_control,
                "Allow remote shutdown/restart/lock from phone",
            )
            .changed()
        {
            changed = true;
        }
        if changed {
            settings.block_alt_tab = state.settings.block_alt_tab;
            settings.sites_blocked = state.settings.sites_blocked;
            settings.pin = state.settings.pin.clone();
            settings.port = state.settings.port;
            settings.escape_exits_fullscreen = state.settings.escape_exits_fullscreen;
            settings.start_minimized = state.settings.start_minimized;
            settings.close_to_tray = state.settings.close_to_tray;
            settings.remote_import = state.settings.remote_import;
            if let Err(error) = state.update_settings(settings) {
                state.error_message = Some(error);
            } else {
                state.error_message = None;
            }
        }
    }

    fn show_notes_section(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        ui.horizontal(|ui| {
            ui.heading("Notes");
            ui.label(
                egui::RichText::new(format!("{} total", state.notes.entries.len()))
                    .size(12.0)
                    .color(theme::MUTED),
            );
        });
        ui.add_space(6.0);

        if state.notes.entries.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Finish a focus session or add a note from the phone to see it here.",
                )
                .color(theme::MUTED),
            );
            return;
        }

        struct Row {
            id: String,
            accomplishment: String,
            subject: Option<String>,
            duration_seconds: u64,
            updated_at: Option<String>,
        }

        let rows: Vec<Row> = state
            .notes
            .entries
            .iter()
            .rev()
            .map(|n| Row {
                id: n.id.clone(),
                accomplishment: n.accomplishment.clone(),
                subject: n.subject.clone(),
                duration_seconds: n.duration_seconds,
                updated_at: n.updated_at.clone(),
            })
            .collect();

        let mut pending_edit: Option<(String, String, String)> = None;
        let mut pending_delete: Option<String> = None;
        let mut cancel_edit = false;

        for row in &rows {
            let editing_this = self.editing_note.as_ref().is_some_and(|e| e.id == row.id);

            egui::Frame::new()
                .fill(theme::PANEL)
                .corner_radius(16.0)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    if editing_this {
                        let editing = self.editing_note.as_mut().unwrap();

                        ui.label(
                            egui::RichText::new("Editing note")
                                .size(11.0)
                                .strong()
                                .color(theme::MUTED),
                        );
                        ui.add_space(4.0);

                        ui.label(
                            egui::RichText::new("Accomplishment")
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut editing.accomplishment)
                                .id(egui::Id::new(("note-edit-acc", &editing.id))),
                        );

                        ui.label(
                            egui::RichText::new("Subject")
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut editing.subject)
                                .id(egui::Id::new(("note-edit-subj", &editing.id))),
                        );

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.push_id(("note-save-btn", &editing.id), |ui| {
                                if ui.button("Save").clicked() {
                                    pending_edit = Some((
                                        editing.id.clone(),
                                        editing.accomplishment.clone(),
                                        editing.subject.clone(),
                                    ));
                                }
                            });
                            ui.push_id(("note-cancel-btn", &editing.id), |ui| {
                                if ui.button("Cancel").clicked() {
                                    cancel_edit = true;
                                }
                            });
                        });
                    } else {
                        ui.horizontal(|ui| {
                            if let Some(subject) = row
                                .subject
                                .as_deref()
                                .filter(|s| !s.trim().is_empty())
                            {
                                ui.label(
                                    egui::RichText::new(subject)
                                        .size(11.0)
                                        .strong()
                                        .color(Color32::from_rgb(129, 201, 149)),
                                );
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.push_id(("note-delete-btn", &row.id), |ui| {
                                        if ui.button("Delete").clicked() {
                                            pending_delete = Some(row.id.clone());
                                        }
                                    });
                                    ui.push_id(("note-edit-btn", &row.id), |ui| {
                                        if ui.button("Edit").clicked() {
                                            let already_editing = self
                                                .editing_note
                                                .as_ref()
                                                .is_some_and(|e| e.id == row.id);
                                            if !already_editing {
                                                self.editing_note = Some(EditingNote {
                                                    id: row.id.clone(),
                                                    accomplishment: row.accomplishment.clone(),
                                                    subject: row
                                                        .subject
                                                        .clone()
                                                        .unwrap_or_default(),
                                                });
                                            }
                                        }
                                    });
                                },
                            );
                        });
                        ui.label(egui::RichText::new(&row.accomplishment).color(theme::TEXT));
                        if row.duration_seconds > 0 {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} · {}",
                                    readable_duration(row.duration_seconds),
                                    if row.updated_at.is_some() { "edited" } else { "logged" }
                                ))
                                .size(11.0)
                                .color(theme::MUTED),
                            );
                        }
                    }
                });
            ui.add_space(6.0);
        }

        if cancel_edit {
            self.editing_note = None;
        }

        if let Some((id, accomplishment, subject)) = pending_edit {
            let subject_opt = if subject.trim().is_empty() {
                None
            } else {
                Some(subject)
            };
            match state.update_note(&id, accomplishment, subject_opt) {
                Ok(()) => {
                    self.editing_note = None;
                    state.error_message = None;
                }
                Err(error) => state.error_message = Some(error),
            }
        }

        if let Some(id) = pending_delete {
            match state.delete_note(&id) {
                Ok(()) => {
                    state.error_message = None;
                    self.undo_started = Some(Instant::now());
                }
                Err(error) => state.error_message = Some(error),
            }
        }
    }

    fn show_note_popup(&mut self, ctx: &egui::Context, state: &mut AppState) {
        let Some(pending_duration) = state.pending_note_duration else {
            return;
        };
        egui::Window::new("What did you accomplish?")
            .id(Id::new("study-note-popup"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("A quick note makes it easier to see what your focus added up to.");
                ui.add_space(8.0);
                ui.label("Accomplishment");
                ui.add(
                    egui::TextEdit::singleline(&mut self.note_text)
                        .id(egui::Id::new("popup-acc")),
                );
                ui.add_space(4.0);
                ui.label("Subject tag (optional)");
                ui.add(
                    egui::TextEdit::singleline(&mut self.subject_text)
                        .id(egui::Id::new("popup-subj")),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Save note").clicked() {
                        match state.save_note(
                            self.note_text.clone(),
                            Some(self.subject_text.clone()),
                            pending_duration,
                        ) {
                            Ok(_) => {
                                self.note_text.clear();
                                self.subject_text.clear();
                                state.pending_note_duration = None;
                                state.error_message = None;
                            }
                            Err(error) => state.error_message = Some(error),
                        }
                    }
                    if ui.button("Skip").clicked() {
                        state.pending_note_duration = None;
                        self.note_text.clear();
                        self.subject_text.clear();
                    }
                });
            });
    }

    fn show_undo_toast(&mut self, ctx: &egui::Context, state: &mut AppState) {
        let Some(started) = self.undo_started else {
            return;
        };
        if started.elapsed() >= Duration::from_secs(30) || !state.undo_available() {
            self.undo_started = None;
            return;
        }

        let seconds_left = 30u64.saturating_sub(started.elapsed().as_secs());
        egui::Area::new(Id::new("undo-toast"))
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -32.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(45, 55, 45))
                    .corner_radius(12.0)
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("Note deleted · {seconds_left}s"))
                                    .color(theme::TEXT),
                            );
                            if ui.button("Undo").clicked() {
                                match state.undo_delete_note() {
                                    Ok(()) => {
                                        state.error_message = None;
                                        self.undo_started = None;
                                    }
                                    Err(error) => state.error_message = Some(error),
                                }
                            }
                        });
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn show_power_banner(&mut self, ctx: &egui::Context, state: &mut AppState) {
        let (Some(action), Some(seconds)) = (
            state.pending_power_action.as_ref().map(|p| p.action),
            state.pending_power_seconds(),
        ) else {
            return;
        };

        let label = match action {
            PowerAction::Shutdown => "Shutdown",
            PowerAction::Restart => "Restart",
        };

        egui::Area::new(Id::new("pending-power"))
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 24.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(120, 40, 40))
                    .corner_radius(12.0)
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{label} in {seconds}s"))
                                    .strong()
                                    .color(theme::TEXT),
                            );
                            if ui.button("Cancel").clicked() {
                                match state.cancel_power_action() {
                                    Ok(()) => state.error_message = None,
                                    Err(error) => state.error_message = Some(error),
                                }
                            }
                        });
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn show_import_dialog(&mut self, ctx: &egui::Context, state: &mut AppState) {
        if !self.show_import_dialog {
            return;
        }
        let mut close = false;
        egui::Window::new("Import data")
            .id(Id::new("import-dialog"))
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(
                    "Paste the full path to an `all.json` file exported from this app.",
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.import_path)
                        .hint_text("C:\\Users\\You\\focusv2\\exports\\2026-10-18-1200\\all.json")
                        .desired_width(f32::INFINITY)
                        .id(egui::Id::new("import-path")),
                );
                ui.add_space(8.0);

                ui.label("Mode");
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.import_mode,
                        import::Mode::Merge,
                        "Merge (keep existing, add missing)",
                    );
                    ui.radio_value(
                        &mut self.import_mode,
                        import::Mode::Replace,
                        "Replace (wipe existing)",
                    );
                });

                if self.import_mode == import::Mode::Replace {
                    ui.label(
                        egui::RichText::new(
                            "⚠ Replace will erase your current history and notes.",
                        )
                        .color(Color32::from_rgb(243, 139, 168))
                        .size(12.0),
                    );
                }
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("Preview").clicked() {
                        self.try_import_preview(state);
                    }
                    if ui.button("Apply").clicked() {
                        self.try_import_apply(state);
                        close = true;
                    }
                });
            });
        if close {
            self.show_import_dialog = false;
        }
    }

    fn try_import_preview(&self, state: &mut AppState) {
        let path = self.import_path.trim();
        if path.is_empty() {
            state.error_message = Some("Enter a file path first".to_owned());
            return;
        }
        match std::fs::read(path) {
            Ok(bytes) => match import::parse(&bytes) {
                Ok(bundle) => {
                    let preview = import::preview(&bundle, &state.history, &state.notes);
                    state.error_message = Some(format!(
                        "Would import: {} days ({}), {} notes — {} days and {} notes overlap",
                        preview.history_days,
                        readable_duration(preview.history_seconds),
                        preview.notes,
                        preview.overlapping_days.len(),
                        preview.overlapping_note_ids.len(),
                    ));
                }
                Err(error) => state.error_message = Some(error),
            },
            Err(error) => {
                state.error_message = Some(format!("Could not read {path}: {error}"));
            }
        }
    }

    fn try_import_apply(&self, state: &mut AppState) {
        let path = self.import_path.trim();
        if path.is_empty() {
            state.error_message = Some("Enter a file path first".to_owned());
            return;
        }
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                state.error_message = Some(format!("Could not read {path}: {error}"));
                return;
            }
        };
        let bundle = match import::parse(&bytes) {
            Ok(bundle) => bundle,
            Err(error) => {
                state.error_message = Some(error);
                return;
            }
        };

        let previous_history = state.history.clone();
        let previous_notes = state.notes.clone();

        let mut history = std::mem::take(&mut state.history);
        let mut notes = std::mem::take(&mut state.notes);
        let report = import::apply(bundle, &mut history, &mut notes, self.import_mode);
        state.history = history;
        state.notes = notes;

        if let Err(error) = state.storage.save_history(&state.history) {
            state.history = previous_history;
            state.notes = previous_notes;
            state.error_message = Some(format!("Could not save history: {error}"));
            return;
        }
        if let Err(error) = state.storage.save_notes(&state.notes) {
            state.history = previous_history;
            state.notes = previous_notes;
            let _ = state.storage.save_history(&state.history);
            state.error_message = Some(format!("Could not save notes: {error}"));
            return;
        }

        state.error_message = Some(format!(
            "Imported: +{} days, {} days updated, +{} notes, {} notes skipped",
            report.history_days_added,
            report.history_days_updated,
            report.notes_added,
            report.notes_skipped,
        ));
        state.ctx.request_repaint();
    }
}

fn setting_control(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).size(12.0).color(theme::MUTED));
        changed |= ui
            .add(
                egui::DragValue::new(value)
                    .range(range)
                    .speed(0.1)
                    .suffix(" min"),
            )
            .changed();
    });
    changed
}

fn action_button(
    ui: &mut egui::Ui,
    label: &str,
    fill: Color32,
    text: Color32,
    width: f32,
) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).strong().color(text))
            .fill(fill)
            .corner_radius(20.0)
            .min_size(Vec2::new(width, 48.0)),
    )
}

fn ring_point(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    center + Vec2::new(angle.cos() * radius, angle.sin() * radius)
}

fn mix_color(from: Color32, to: Color32, amount: f32) -> Color32 {
    let blend = |start: u8, end: u8| (start as f32 + (end as f32 - start as f32) * amount) as u8;
    Color32::from_rgba_unmultiplied(
        blend(from.r(), to.r()),
        blend(from.g(), to.g()),
        blend(from.b(), to.b()),
        blend(from.a(), to.a()),
    )
}

fn readable_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = seconds % 3600 / 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

fn load_window_icon() -> Option<std::sync::Arc<egui::IconData>> {
    let bytes = include_bytes!("../icon.png");
    match eframe::icon_data::from_png_bytes(bytes) {
        Ok(data) => Some(std::sync::Arc::new(data)),
        Err(error) => {
            eprintln!("Could not load window icon: {error}");
            None
        }
    }
}

pub fn native_options(settings: &crate::storage::Settings) -> eframe::NativeOptions {
    let icon = load_window_icon();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([900.0, 700.0])
        .with_title("Pomodoro Focus")
        .with_visible(!settings.start_minimized);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    eframe::NativeOptions {
        viewport,
        ..Default::default()
    }
}

pub fn create_app(ctx: &eframe::CreationContext<'_>) -> Result<PomodoroApp, getrandom::Error> {
    PomodoroApp::new(ctx)
}