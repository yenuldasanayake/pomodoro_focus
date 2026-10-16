use crate::{
    alt_tab, charts, site_blocker,
    server::{self, AppState, SharedState},
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
}

impl PomodoroApp {
    pub fn new(ctx: &eframe::CreationContext<'_>) -> Result<Self, getrandom::Error> {
        let storage = Storage::new(".");
        let pin = server::generate_pin()?;
        let url = server::local_url();
        let state = AppState::new(ctx.egui_ctx.clone(), storage, pin, url);
        let previous_mode = state.timer.mode;
        let transition_from = theme::accent(previous_mode);
        let shared = Arc::new(Mutex::new(state));
        server::start_server(shared.clone());
        Ok(Self {
            shared,
            note_text: String::new(),
            subject_text: String::new(),
            previous_mode,
            transition_from,
            transition_started: Instant::now(),
        })
    }
}

impl eframe::App for PomodoroApp {
    fn ui(&mut self, root_ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root_ui.ctx().clone();
        theme::apply(&ctx);

        let shared = self.shared.clone();
        let Ok(mut state) = shared.lock() else {
            egui::CentralPanel::default().show(root_ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label("The timer state is unavailable. Please restart the app.");
                });
            });
            return;
        };

        if state.settings.escape_exits_fullscreen
            && ctx.input(|input| input.key_pressed(Key::Escape))
        {
            let is_fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!is_fullscreen));
        }

        // Keyboard shortcuts, skipped while a text field has focus.
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
                    });
            });

        self.show_note_popup(&ctx, &mut state);

        if let Some(message) = &state.server_error {
            egui::Area::new(Id::new("server-error"))
                .anchor(Align2::RIGHT_TOP, Vec2::new(-24.0, 24.0))
                .show(&ctx, |ui| {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(69, 35, 49))
                        .corner_radius(12)
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
                        egui::RichText::new(message)
                            .color(Color32::from_rgb(243, 139, 168)),
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
            ui.add_space(24.0);
            ui.label(
                egui::RichText::new("◉  POMODORO FOCUS")
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
                ui.add_space(24.0);
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
            Stroke::new(5.0, Color32::from_rgb(49, 50, 68)),
        );

        let progress = state.timer.progress(&state.settings);
        let steps = ((progress * 200.0).ceil() as usize).max(1);
        if progress > 0.0 {
            let mut previous = ring_point(center, radius, -std::f32::consts::FRAC_PI_2);
            for step in 1..=steps {
                let angle = -std::f32::consts::FRAC_PI_2
                    + std::f32::consts::TAU * progress * step as f32 / steps as f32;
                let point = ring_point(center, radius, angle);
                painter.line_segment([previous, point], Stroke::new(5.0, accent));
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
        painter.circle_filled(center + Vec2::new(0.0, 116.0), 3.0, accent);
    }

    fn show_controls(&self, ui: &mut egui::Ui, state: &mut AppState, accent: Color32) {
        ui.horizontal(|ui| {
            if action_button(ui, "Skip", Color32::from_rgb(49, 50, 68), theme::TEXT, 76.0)
                .clicked()
                && let Err(error) = state.execute_command("skip")
            {
                state.error_message = Some(error);
            }
            ui.add_space(12.0);
            let label = if state.timer.running {
                "Pause"
            } else {
                "Start focus"
            };
            if action_button(ui, label, accent, Color32::from_rgb(24, 24, 37), 176.0).clicked() {
                let command = if state.timer.running {
                    "pause"
                } else {
                    "start"
                };
                if let Err(error) = state.execute_command(command) {
                    state.error_message = Some(error);
                }
            }
            ui.add_space(12.0);
            if action_button(ui, "Reset", Color32::from_rgb(49, 50, 68), theme::TEXT, 76.0)
                .clicked()
                && let Err(error) = state.execute_command("reset")
            {
                state.error_message = Some(error);
            }
        });
    }

    fn show_connection(&self, ui: &mut egui::Ui, state: &mut AppState) {
        ui.add_space(12.0);
        egui::Frame::new()
            .fill(theme::PANEL)
            .corner_radius(16)
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
                            ui.label(egui::RichText::new(&state.local_url).color(theme::TEXT));
                            ui.label(
                                egui::RichText::new(format!(" · PIN  {}", state.pin))
                                    .strong()
                                    .color(theme::TEXT),
                            );
                        });
                    });
                    if ui.button("Copy URL").clicked() {
                        ui.ctx().copy_text(state.local_url.clone());
                    }
                });
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
                egui::RichText::new("Changes save automatically")
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
        });
        if changed {
            if let Err(error) = state.update_settings(settings) {
                state.error_message = Some(error);
            } else {
                state.error_message = None;
            }
        }
    }

    fn show_note_popup(&mut self, ctx: &egui::Context, state: &mut AppState) {
        if state.pending_note_duration.is_none() {
            return;
        }
        egui::Window::new("What did you accomplish?")
            .id(Id::new("study-note-popup"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(
                    "A quick note makes it easier to see what your focus added up to.",
                );
                ui.add_space(8.0);
                ui.label("Accomplishment");
                ui.text_edit_singleline(&mut self.note_text);
                ui.add_space(4.0);
                ui.label("Subject tag (optional)");
                ui.text_edit_singleline(&mut self.subject_text);
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Save note").clicked() {
                        match state.save_note(
                            std::mem::take(&mut self.note_text),
                            Some(std::mem::take(&mut self.subject_text)),
                        ) {
                            Ok(()) => state.error_message = None,
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
            .corner_radius(14)
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

pub fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_fullscreen(true)
            .with_title("Pomodoro Focus"),
        ..Default::default()
    }
}

pub fn create_app(ctx: &eframe::CreationContext<'_>) -> Result<PomodoroApp, getrandom::Error> {
    PomodoroApp::new(ctx)
}