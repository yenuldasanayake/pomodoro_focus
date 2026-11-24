use crate::{
    breaks, charts, export, import,
    server::{self, AppState, PowerAction, SharedState},
    storage::Storage,
    theme,
    timer::Mode,
    tray,
};
use chrono::{DateTime, Duration as CalendarDuration, Local, NaiveDate};
use eframe::egui::{self, Align2, Color32, FontId, Id, Key, Pos2, Sense, Stroke, Vec2};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const SURFACE_BORDER: Color32 = Color32::from_rgb(54, 58, 65);
const SECONDARY_FILL: Color32 = Color32::from_rgb(38, 41, 47);
const SECONDARY_BORDER: Color32 = Color32::from_rgb(64, 69, 77);
const SUCCESS_GREEN: Color32 = Color32::from_rgb(129, 201, 149);
const ERROR_ROSE: Color32 = Color32::from_rgb(243, 139, 168);
const WARN_AMBER: Color32 = Color32::from_rgb(250, 179, 135);
const HERO_DARK_TEXT: Color32 = Color32::from_rgb(18, 20, 23);

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
    task_prompt_text: String,
    reflection_went_well: String,
    reflection_adjust_next_time: String,
    reflection_next_week_focus: String,
    reflection_week_start: String,
    reflection_saved_snapshot: crate::storage::WeeklyReflection,
    setup_study_minutes: u32,
    setup_goal_minutes: u32,
    settings_saved_at: Option<Instant>,
    reflection_saved_at: Option<Instant>,
    show_shortcuts: bool,
    qr_cache: Option<(String, usize, Vec<bool>)>,
    show_day_wrap_up: bool,
    tomorrow_plan_text: String,
    plan_saved_at: Option<Instant>,
    applied_compact_mode: bool,
    applied_always_on_top: bool,
    expanded_window_size: Vec2,
    status_banner: Option<(String, Instant)>,
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
        let task_prompt_text = state.settings.active_task.clone();
        let reflection_went_well = state.current_week_reflection.went_well.clone();
        let reflection_adjust_next_time = state.current_week_reflection.adjust_next_time.clone();
        let reflection_next_week_focus = state.current_week_reflection.next_week_focus.clone();
        let reflection_week_start = state.current_week_reflection.week_start.clone();
        let reflection_saved_snapshot = state.current_week_reflection.clone();
        let setup_study_minutes = state.settings.study_minutes;
        let setup_goal_minutes = state.settings.daily_goal_minutes;
        let tomorrow = (Local::now().date_naive() + CalendarDuration::days(1)).to_string();
        let tomorrow_plan_text = state.history.plan_for(&tomorrow).unwrap_or_default().to_owned();
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
            ctx.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
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
            task_prompt_text,
            reflection_went_well,
            reflection_adjust_next_time,
            reflection_next_week_focus,
            reflection_week_start,
            reflection_saved_snapshot,
            setup_study_minutes,
            setup_goal_minutes,
            settings_saved_at: None,
            reflection_saved_at: None,
            show_shortcuts: false,
            qr_cache: None,
            show_day_wrap_up: false,
            tomorrow_plan_text,
            plan_saved_at: None,
            applied_compact_mode: false,
            applied_always_on_top: false,
            expanded_window_size: Vec2::new(900.0, 700.0),
            status_banner: None,
        })
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status_banner = Some((message.into(), Instant::now()));
    }
}

impl eframe::App for PomodoroApp {
    fn ui(&mut self, root_ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root_ui.ctx().clone();
        theme::apply(&ctx);

        if let Some(tray) = &self.tray {
            let tooltip_state = if let Ok(state) = self.shared.lock() {
                let minutes = state.timer.remaining_seconds / 60;
                let seconds = state.timer.remaining_seconds % 60;
                let status = if state.timer.running { "Running" } else { "Paused" };
                format!("Pomodoro Focus — {status} · {minutes:02}:{seconds:02}")
            } else {
                "Pomodoro Focus".to_owned()
            };
            tray.set_tooltip(&tooltip_state);
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
                minimize_instead = state.settings.close_to_tray;
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

        if self.applied_always_on_top != state.settings.always_on_top {
            let level = if state.settings.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
            self.applied_always_on_top = state.settings.always_on_top;
        }
        if self.applied_compact_mode != state.settings.compact_timer {
            if state.settings.compact_timer {
                self.expanded_window_size = ctx
                    .input(|input| input.viewport().inner_rect.map(|rect| rect.size()))
                    .unwrap_or(Vec2::new(900.0, 700.0));
                ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(Vec2::new(280.0, 220.0)));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(340.0, 260.0)));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(Vec2::new(380.0, 520.0)));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(self.expanded_window_size));
            }
            self.applied_compact_mode = state.settings.compact_timer;
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

        // Resync our local flag from the OS when it reports a definitive value.
        if let Some(v) = ctx.input(|input| input.viewport().fullscreen) {
            self.app_is_fullscreen = v;
        }

        let text_editing = ctx.memory(|memory| memory.focused().is_some());
        if !text_editing && state.settings.onboarding_complete {
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
            if ctx.input(|input| input.key_pressed(Key::F1)) {
                self.show_shortcuts = !self.show_shortcuts;
            }
        }

        state.tick();
        if state.timer.mode != self.previous_mode {
            if state.timer.mode == Mode::Study && self.previous_mode != Mode::Study {
                self.task_prompt_text = state.settings.active_task.clone();
            }
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
            ctx.request_repaint_after(Duration::from_millis(33));
        }

        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BACKGROUND)).show(
            root_ui,
            |ui| {
                if state.settings.compact_timer {
                    self.show_compact_timer(ui, &mut state, accent);
                } else {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(28, 22))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical().auto_shrink([false, false]).show(
                                ui,
                                |ui| {
                                    let content_width = ui.available_width().min(860.0);
                                    ui.vertical_centered(|ui| {
                                        ui.set_max_width(content_width);

                                        self.show_top(ui, &mut state, accent);
                                        ui.add_space(14.0);
                                        self.show_today_summary(ui, &mut state, accent);
                                        ui.add_space(18.0);

                                        self.show_timer(ui, &state, accent);
                                        ui.add_space(14.0);
                                        self.show_controls(ui, &mut state, accent);
                                        ui.add_space(10.0);
                                        self.show_shortcut_bar(ui);

                                        if state.task_prompt_pending {
                                            ui.add_space(14.0);
                                            self.show_task_prompt(ui, &mut state, accent);
                                        }

                                        ui.add_space(22.0);
                                        self.show_more_section(ui, &mut state);
                                    });
                                },
                            );
                        });
                }
            },
        );

        self.show_note_popup(&ctx, &mut state);
        self.show_day_wrap_up(&ctx, &mut state);
        self.show_setup_dialog(&ctx, &mut state);
        self.show_undo_toast(&ctx, &mut state);
        self.show_power_banner(&ctx, &mut state);
        self.show_import_dialog(&ctx, &mut state);
        self.show_shortcut_help(&ctx);

        if let Some((msg, shown_at)) = &self.status_banner {
            if shown_at.elapsed() < Duration::from_secs(6) {
                egui::Area::new(Id::new("status-banner"))
                    .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-24.0, -20.0))
                    .show(&ctx, |ui| {
                        egui::Frame::new()
                            .fill(theme::PANEL)
                            .stroke(Stroke::new(1.0, SUCCESS_GREEN.linear_multiply(0.5)))
                            .corner_radius(12.0)
                            .inner_margin(egui::Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new(msg).size(12.5).color(theme::TEXT));
                            });
                    });
                ctx.request_repaint_after(Duration::from_millis(250));
            } else {
                self.status_banner = None;
            }
        }

        if let Some(message) = &state.server_error {
            egui::Area::new(Id::new("server-error"))
                .anchor(Align2::RIGHT_TOP, Vec2::new(-24.0, 24.0))
                .show(&ctx, |ui| {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(69, 35, 49))
                        .stroke(Stroke::new(1.0, ERROR_ROSE.linear_multiply(0.45)))
                        .corner_radius(14.0)
                        .inner_margin(egui::Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(message).color(theme::TEXT));
                        });
                });
        }
        if let Some(message) = &state.error_message {
            egui::Area::new(Id::new("storage-error"))
                .anchor(Align2::LEFT_BOTTOM, Vec2::new(24.0, -20.0))
                .show(&ctx, |ui| {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(56, 28, 38))
                        .stroke(Stroke::new(1.0, ERROR_ROSE.linear_multiply(0.5)))
                        .corner_radius(12.0)
                        .inner_margin(egui::Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(message).size(12.5).color(ERROR_ROSE));
                        });
                });
        }
    }

    fn on_exit(&mut self) {
        if let Ok(mut state) = self.shared.lock() {
            state.flush_all();
        }
    }
}

impl PomodoroApp {
    fn show_top(&mut self, ui: &mut egui::Ui, state: &mut AppState, accent: Color32) {
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("POMODORO FOCUS")
                    .size(14.0)
                    .strong()
                    .color(theme::TEXT),
            );
            ui.label(
                egui::RichText::new(format!("· {}", state.timer.mode.label()))
                    .size(12.0)
                    .strong()
                    .color(accent),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Compact timer").size(11.5).color(theme::TEXT),
                        )
                        .fill(SECONDARY_FILL)
                        .stroke(Stroke::new(1.0, SECONDARY_BORDER))
                        .corner_radius(8.0),
                    )
                    .clicked()
                {
                    set_compact_timer(state, true);
                }
                ui.add_space(6.0);
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .stroke(Stroke::new(1.0, SURFACE_BORDER))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{:02} COMPLETED",
                                state.timer.completed_study_sessions
                            ))
                            .size(11.0)
                            .strong()
                            .color(theme::MUTED),
                        );
                    });
            });
        });
        ui.add_space(10.0);
        ui.painter().hline(
            ui.available_rect_before_wrap().x_range(),
            ui.cursor().top(),
            Stroke::new(1.0, SURFACE_BORDER),
        );
        ui.add_space(2.0);
    }

    fn show_compact_timer(&mut self, ui: &mut egui::Ui, state: &mut AppState, accent: Color32) {
        egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(state.timer.mode.label()).strong().color(accent));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Expand").clicked() {
                        set_compact_timer(state, false);
                    }
                    ui.label(
                        egui::RichText::new(if state.timer.running { "RUNNING" } else { "PAUSED" })
                            .size(10.0)
                            .color(theme::MUTED),
                    );
                });
            });
            ui.add_space(10.0);
            let minutes = state.timer.remaining_seconds / 60;
            let seconds = state.timer.remaining_seconds % 60;
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(format!("{minutes:02}:{seconds:02}"))
                        .size(56.0)
                        .strong()
                        .color(theme::TEXT),
                );
                ui.add_space(4.0);
                ui.add(
                    egui::ProgressBar::new(state.timer.progress(&state.settings))
                        .desired_width(ui.available_width())
                        .fill(accent),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let label = if state.timer.running { "Pause" } else { "Start" };
                    if ui.button(label).clicked() {
                        let command = if state.timer.running { "pause" } else { "start" };
                        if let Err(error) = state.execute_command(command) {
                            state.error_message = Some(error);
                        }
                    }
                    if ui.button("Skip").clicked()
                        && let Err(error) = state.execute_command("skip")
                    {
                        state.error_message = Some(error);
                    }
                });
            });
        });
    }

    fn show_today_summary(&mut self, ui: &mut egui::Ui, state: &mut AppState, accent: Color32) {
        let today = Local::now().date_naive();
        let today_key = today.to_string();
        let tomorrow_key = (today + CalendarDuration::days(1)).to_string();

        egui::Frame::new()
            .fill(theme::PANEL)
            .stroke(Stroke::new(1.0, SURFACE_BORDER))
            .corner_radius(16.0)
            .inner_margin(egui::Margin::symmetric(20, 16))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Today");
                    if !state.settings.active_task.trim().is_empty() {
                        ui.label(
                            egui::RichText::new(format!("· {}", state.settings.active_task.trim()))
                                .size(13.0)
                                .color(theme::MUTED),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Wrap up day")
                                        .size(11.5)
                                        .color(theme::TEXT),
                                )
                                .fill(SECONDARY_FILL)
                                .stroke(Stroke::new(1.0, SECONDARY_BORDER))
                                .corner_radius(8.0),
                            )
                            .clicked()
                        {
                            self.tomorrow_plan_text = state
                                .history
                                .plan_for(&tomorrow_key)
                                .unwrap_or_default()
                                .to_owned();
                            self.plan_saved_at = None;
                            self.show_day_wrap_up = true;
                        }
                    });
                });

                ui.add_space(10.0);
                ui.columns(3, |columns| {
                    columns[0]
                        .label(egui::RichText::new("FOCUS TIME").size(10.0).strong().color(theme::MUTED));
                    columns[0].label(
                        egui::RichText::new(readable_duration(state.history.today_seconds()))
                            .size(21.0)
                            .strong()
                            .color(theme::TEXT),
                    );

                    columns[1]
                        .label(egui::RichText::new("SESSIONS").size(10.0).strong().color(theme::MUTED));
                    columns[1].label(
                        egui::RichText::new(state.history.today_sessions().to_string())
                            .size(21.0)
                            .strong()
                            .color(theme::TEXT),
                    );
                    if state.history.has_untracked_session_days() {
                        columns[1].label(
                            egui::RichText::new("Older counts weren’t stored; tracking from now.")
                                .size(10.0)
                                .color(theme::MUTED),
                        );
                    }

                    columns[2]
                        .label(egui::RichText::new("DAILY GOAL").size(10.0).strong().color(theme::MUTED));
                    if state.settings.daily_goal_minutes == 0 {
                        columns[2]
                            .label(egui::RichText::new("Not set").size(16.0).color(theme::MUTED));
                    } else {
                        let goal_seconds = u64::from(state.settings.daily_goal_minutes) * 60;
                        let today_seconds = state.history.today_seconds();
                        let ratio = (today_seconds as f32 / goal_seconds as f32).clamp(0.0, 1.0);
                        columns[2].label(
                            egui::RichText::new(format!(
                                "{} / {}",
                                readable_duration(today_seconds),
                                readable_duration(goal_seconds)
                            ))
                            .size(16.0)
                            .strong()
                            .color(theme::TEXT),
                        );
                        columns[2].add_space(2.0);
                        columns[2].add(
                            egui::ProgressBar::new(ratio)
                                .desired_width(150.0)
                                .fill(if ratio >= 1.0 { SUCCESS_GREEN } else { accent }),
                        );
                    }
                });

                let weekly_days = state.history.weekly_focus_days();
                let weekly_goal = state.settings.weekly_goal_days.max(1);
                let weekly_streak = state.history.weekly_streak(state.settings.weekly_goal_days);
                let weekly_ratio = (weekly_days as f32 / weekly_goal as f32).clamp(0.0, 1.0);

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "This week · {weekly_days}/{} focus days · {weekly_streak}-week rhythm · rest days are okay",
                            state.settings.weekly_goal_days
                        ))
                        .size(12.0)
                        .color(theme::MUTED),
                    );
                });
                ui.add_space(4.0);
                ui.add(
                    egui::ProgressBar::new(weekly_ratio)
                        .desired_width(ui.available_width())
                        .fill(if weekly_ratio >= 1.0 { SUCCESS_GREEN } else { accent }),
                );

                if let Some(plan) = state.history.plan_for(&today_key) {
                    if !plan.trim().is_empty() {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(format!("Today’s first step · {plan}"))
                                .size(12.0)
                                .color(theme::TEXT),
                        );
                    }
                }
            });
    }

    fn show_timer(&self, ui: &mut egui::Ui, state: &AppState, accent: Color32) {
        let diameter = ui.available_width().min(414.0).max(180.0);
        let scale = diameter / 414.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::hover());
        let painter = ui.painter_at(rect);
        let center = rect.center();
        let radius = diameter * 0.43;

        // Subtle inner backdrop + track ring
        painter.circle_filled(center, radius, Color32::from_rgb(30, 32, 36));
        painter.circle_stroke(center, radius, Stroke::new(7.0, Color32::from_rgb(56, 60, 66)));

        let progress = state.timer.progress(&state.settings);
        let steps = ((progress * 240.0).ceil() as usize).max(1);
        if progress > 0.0 {
            let mut previous = ring_point(center, radius, -std::f32::consts::FRAC_PI_2);
            for step in 1..=steps {
                let angle = -std::f32::consts::FRAC_PI_2
                    + std::f32::consts::TAU * progress * step as f32 / steps as f32;
                let point = ring_point(center, radius, angle);
                painter.line_segment([previous, point], Stroke::new(7.0, accent));
                previous = point;
            }
        }

        painter.text(
            center + Vec2::new(0.0, -72.0 * scale),
            Align2::CENTER_CENTER,
            format!(
                "{} · {}",
                state.timer.mode.label(),
                if state.timer.running { "RUNNING" } else { "PAUSED" }
            ),
            FontId::proportional((13.5 * scale).max(10.0)),
            accent,
        );

        let minutes = state.timer.remaining_seconds / 60;
        let seconds = state.timer.remaining_seconds % 60;
        painter.text(
            center + Vec2::new(0.0, 4.0 * scale),
            Align2::CENTER_CENTER,
            format!("{minutes:02}:{seconds:02}"),
            FontId::proportional((96.0 * scale).max(42.0)),
            theme::TEXT,
        );

        if let Some(suggestion) =
            breaks::current_suggestion(state.timer.mode, state.timer.elapsed_in_mode)
        {
            painter.text(
                center + Vec2::new(0.0, 74.0 * scale),
                Align2::CENTER_CENTER,
                format!("💡 {suggestion}"),
                FontId::proportional((12.5 * scale).max(10.0)),
                theme::MUTED,
            );
        } else {
            painter.text(
                center + Vec2::new(0.0, 74.0 * scale),
                Align2::CENTER_CENTER,
                format!("TODAY  {}", readable_duration(state.history.today_seconds())),
                FontId::proportional((13.5 * scale).max(10.0)),
                theme::MUTED,
            );
        }

        if state.settings.daily_goal_minutes > 0 {
            let goal_secs = u64::from(state.settings.daily_goal_minutes) * 60;
            let today = state.history.today_seconds();
            let progress = (today as f32 / goal_secs as f32).clamp(0.0, 1.0);
            let bar_width = radius * 1.35;
            let bar_height = 6.0;
            let bar_rect = egui::Rect::from_center_size(
                center + Vec2::new(0.0, 114.0 * scale),
                Vec2::new(bar_width, bar_height),
            );
            painter.rect_filled(bar_rect, 3.0, Color32::from_rgb(56, 60, 66));
            let filled = egui::Rect::from_min_size(
                bar_rect.min,
                Vec2::new(bar_width * progress, bar_height),
            );
            let fill_color = if progress >= 1.0 { SUCCESS_GREEN } else { accent };
            painter.rect_filled(filled, 3.0, fill_color);
            painter.text(
                center + Vec2::new(0.0, 130.0 * scale),
                Align2::CENTER_CENTER,
                format!("GOAL  {} / {}", readable_duration(today), readable_duration(goal_secs)),
                FontId::proportional((11.0 * scale).max(9.0)),
                theme::MUTED,
            );
        }
    }

    fn show_controls(&self, ui: &mut egui::Ui, state: &mut AppState, accent: Color32) {
        let max_width = ui.available_width().min(440.0);
        let gap = 10.0;
        let side_width = (max_width * 0.18).clamp(52.0, 78.0);
        let main_width = (max_width - side_width * 2.0 - gap * 2.0).max(110.0);

        ui.allocate_ui_with_layout(
            Vec2::new(max_width, 48.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                if secondary_action_button(ui, "Skip", side_width).clicked()
                    && let Err(error) = state.execute_command("skip")
                {
                    state.error_message = Some(error);
                }
                ui.add_space(gap);
                let label = if state.timer.running { "Pause" } else { "Start focus" };
                if action_button(ui, label, accent, HERO_DARK_TEXT, main_width).clicked() {
                    let command = if state.timer.running { "pause" } else { "start" };
                    if let Err(error) = state.execute_command(command) {
                        state.error_message = Some(error);
                    }
                }
                ui.add_space(gap);
                if secondary_action_button(ui, "Reset", side_width).clicked()
                    && let Err(error) = state.execute_command("reset")
                {
                    state.error_message = Some(error);
                }
            },
        );
    }

    fn show_shortcut_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Space · start/pause   ·   R · reset   ·   S · skip   ·")
                    .size(11.5)
                    .color(theme::MUTED),
            );
            let help_resp = ui.add(
                egui::Label::new(
                    egui::RichText::new("Keyboard help")
                        .size(11.5)
                        .underline()
                        .color(theme::MUTED),
                )
                .sense(Sense::click()),
            );
            if help_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if help_resp.clicked() {
                self.show_shortcuts = true;
            }
        });
    }

    fn show_more_section(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        egui::Frame::new()
            .fill(theme::PANEL)
            .stroke(Stroke::new(1.0, SURFACE_BORDER))
            .corner_radius(16.0)
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                egui::CollapsingHeader::new(
                    egui::RichText::new("More").size(14.0).strong().color(theme::TEXT),
                )
                .default_open(false)
                .show(ui, |ui| {
                    ui.add_space(6.0);
                    egui::CollapsingHeader::new("Phone remote & backups")
                        .default_open(false)
                        .show(ui, |ui| {
                            self.show_connection(ui, state);
                        });
                    ui.add_space(4.0);
                    egui::CollapsingHeader::new("Preferences")
                        .default_open(false)
                        .show(ui, |ui| {
                            self.show_settings(ui, state);
                        });
                    ui.add_space(4.0);
                    egui::CollapsingHeader::new("Weekly reflection")
                        .default_open(false)
                        .show(ui, |ui| {
                            self.show_weekly_reflection(ui, state);
                        });
                    ui.add_space(4.0);
                    egui::CollapsingHeader::new("History & charts")
                        .default_open(false)
                        .show(ui, |ui| {
                            charts::show(
                                ui,
                                &state.history,
                                &state.notes,
                                state.settings.weekly_goal_days,
                            );
                        });
                    ui.add_space(4.0);
                    egui::CollapsingHeader::new(format!("Notes · {}", state.notes.entries.len()))
                        .default_open(false)
                        .show(ui, |ui| {
                            self.show_notes_section(ui, state);
                        });
                });
            });
    }

    fn show_day_wrap_up(&mut self, ctx: &egui::Context, state: &mut AppState) {
        if !self.show_day_wrap_up {
            return;
        }

        let today = Local::now().date_naive();
        let tomorrow_key = (today + CalendarDuration::days(1)).to_string();
        let accomplishments: Vec<_> = state
            .notes
            .entries
            .iter()
            .filter(|note| local_note_date(note) == Some(today))
            .cloned()
            .collect();
        let mut open = self.show_day_wrap_up;
        let mut save_plan = false;
        let mut dismiss = false;
        egui::Window::new("A gentle look back")
            .id(Id::new("day-wrap-up"))
            .collapsible(false)
            .resizable(true)
            .open(&mut open)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Today · {} focused · {} completed sessions",
                    readable_duration(state.history.today_seconds()),
                    state.history.today_sessions()
                ));
                ui.add_space(8.0);
                ui.heading("Accomplishments");
                if accomplishments.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "No notes saved today. That’s okay—your focus time still counts.",
                        )
                        .color(theme::MUTED),
                    );
                } else {
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        for note in &accomplishments {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new("•").color(SUCCESS_GREEN));
                                ui.label(&note.accomplishment);
                                if let Some(subject) =
                                    note.subject.as_deref().filter(|value| !value.trim().is_empty())
                                {
                                    ui.label(
                                        egui::RichText::new(format!("· {subject}"))
                                            .size(11.0)
                                            .color(theme::MUTED),
                                    );
                                }
                            });
                        }
                    });
                }
                ui.add_space(10.0);
                ui.heading("A small first step for tomorrow");
                ui.add(
                    egui::TextEdit::singleline(&mut self.tomorrow_plan_text)
                        .desired_width(f32::INFINITY)
                        .hint_text("Optional—keep it simple"),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Save plan").clicked() {
                        save_plan = true;
                    }
                    if ui.button("Done").clicked() {
                        dismiss = true;
                    }
                    if self
                        .plan_saved_at
                        .is_some_and(|saved| saved.elapsed() < Duration::from_secs(3))
                    {
                        ui.label(
                            egui::RichText::new(format!("Saved for {tomorrow_key}"))
                                .color(SUCCESS_GREEN),
                        );
                    }
                });
            });

        if dismiss {
            open = false;
        }
        if save_plan {
            match state.save_daily_plan(tomorrow_key, self.tomorrow_plan_text.clone()) {
                Ok(()) => {
                    self.plan_saved_at = Some(Instant::now());
                    state.error_message = None;
                }
                Err(error) => {
                    self.plan_saved_at = None;
                    state.error_message = Some(error);
                }
            }
        }
        self.show_day_wrap_up = open;
    }

    fn show_weekly_reflection(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        if self.reflection_saved_snapshot != state.current_week_reflection {
            self.reflection_saved_snapshot = state.current_week_reflection.clone();
            self.reflection_week_start = state.current_week_reflection.week_start.clone();
            self.reflection_went_well = state.current_week_reflection.went_well.clone();
            self.reflection_adjust_next_time =
                state.current_week_reflection.adjust_next_time.clone();
            self.reflection_next_week_focus = state.current_week_reflection.next_week_focus.clone();
        }
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(SECONDARY_FILL)
            .stroke(Stroke::new(1.0, SURFACE_BORDER))
            .corner_radius(14.0)
            .inner_margin(egui::Margin::symmetric(18, 14))
            .show(ui, |ui| {
                ui.heading("Weekly reflection");
                ui.label(
                    egui::RichText::new(format!(
                        "Week of {} · {} focused",
                        state.current_week_reflection.week_start,
                        readable_duration(state.history.current_week_seconds())
                    ))
                    .size(12.0)
                    .color(theme::MUTED),
                );
                ui.add_space(8.0);
                ui.label("What went well?");
                ui.add(
                    egui::TextEdit::multiline(&mut self.reflection_went_well)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .hint_text("A small win counts."),
                );
                ui.add_space(4.0);
                ui.label("What would you adjust next time?");
                ui.add(
                    egui::TextEdit::multiline(&mut self.reflection_adjust_next_time)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .hint_text("Keep it practical and kind."),
                );
                ui.add_space(4.0);
                ui.label("One focus for next week");
                ui.add(
                    egui::TextEdit::singleline(&mut self.reflection_next_week_focus)
                        .desired_width(f32::INFINITY)
                        .hint_text("A direction, not a pressure."),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save reflection").clicked() {
                        let reflection = crate::storage::WeeklyReflection {
                            week_start: state.current_week_reflection.week_start.clone(),
                            went_well: self.reflection_went_well.clone(),
                            adjust_next_time: self.reflection_adjust_next_time.clone(),
                            next_week_focus: self.reflection_next_week_focus.clone(),
                        };
                        match state.save_weekly_reflection(reflection) {
                            Ok(()) => {
                                self.reflection_saved_snapshot =
                                    state.current_week_reflection.clone();
                                self.reflection_saved_at = Some(Instant::now());
                                state.error_message = None;
                            }
                            Err(error) => {
                                self.reflection_saved_at = None;
                                state.error_message = Some(error);
                            }
                        }
                    }
                    if self
                        .reflection_saved_at
                        .is_some_and(|saved| saved.elapsed() < Duration::from_secs(3))
                    {
                        ui.label(egui::RichText::new("Saved").color(SUCCESS_GREEN));
                    }
                });
            });
    }

    fn show_connection(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(SECONDARY_FILL)
            .stroke(Stroke::new(1.0, SURFACE_BORDER))
            .corner_radius(14.0)
            .inner_margin(egui::Margin::symmetric(18, 14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("PHONE REMOTE")
                                .size(11.0)
                                .strong()
                                .color(theme::MUTED),
                        );
                        ui.add_space(2.0);
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
                            self.set_status("Remote URL copied to clipboard");
                        }
                        if ui.button("Copy PIN").clicked() {
                            ui.ctx().copy_text(state.pin.clone());
                            self.set_status("Remote PIN copied to clipboard");
                        }
                    });
                });

                if !state.local_url.is_empty() {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        show_qr_code(ui, &state.local_url, &mut self.qr_cache);
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label("Scan to open the phone remote on this Wi-Fi.");
                            ui.label("Then enter the PIN shown above.");
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(
                                    "The QR code contains only the local address, not your PIN.",
                                )
                                .size(12.0)
                                .color(theme::MUTED),
                            );
                        });
                    });
                }

                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("New PIN").clicked() {
                        match state.rotate_pin() {
                            Ok(()) => {
                                state.error_message = None;
                                self.set_status(format!("Rotated PIN to {}", state.pin));
                            }
                            Err(error) => state.error_message = Some(error),
                        }
                    }
                    if ui.button("New port").clicked() {
                        match state.request_new_port() {
                            Ok(()) => {
                                state.error_message = None;
                                self.set_status("Requested new port (restart app to apply)");
                            }
                            Err(error) => state.error_message = Some(error),
                        }
                    }
                    if ui.button("Auto port").clicked() {
                        match state.request_auto_port() {
                            Ok(()) => {
                                state.error_message = None;
                                self.set_status("Enabled automatic port selection");
                            }
                            Err(error) => state.error_message = Some(error),
                        }
                    }
                    if ui.button("Export data…").clicked() {
                        let root = std::path::Path::new(".");
                        match export::write_to_disk(
                            root,
                            &state.history,
                            &state.notes,
                            &state.reflections,
                        ) {
                            Ok(path) => {
                                let display = path.display().to_string();
                                ui.ctx().copy_text(display.clone());
                                state.error_message = None;
                                self.set_status(format!(
                                    "Exported to {display} (path copied to clipboard)"
                                ));
                            }
                            Err(error) => {
                                state.error_message = Some(format!("Export failed: {error}"));
                            }
                        }
                    }
                    if ui.button("Import data…").clicked() {
                        self.import_path.clear();
                        self.show_import_dialog = true;
                    }
                });

                if state.port_change_pending {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("Restart the app to apply the new port.")
                            .size(12.0)
                            .color(WARN_AMBER),
                    );
                }

                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "Open this address on a phone connected to the same Wi-Fi.",
                    )
                    .size(12.0)
                    .color(theme::MUTED),
                );
            });
    }

    fn show_settings(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(SECONDARY_FILL)
            .stroke(Stroke::new(1.0, SURFACE_BORDER))
            .corner_radius(14.0)
            .inner_margin(egui::Margin::symmetric(18, 14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Session settings");
                    ui.label(
                        egui::RichText::new("Changes save automatically · window options below")
                            .size(12.0)
                            .color(theme::MUTED),
                    );
                });
                ui.add_space(8.0);

                let mut settings = state.settings.clone();
                let mut changed = false;
                ui.horizontal_wrapped(|ui| {
                    changed |=
                        setting_control(ui, "Focus · min", &mut settings.study_minutes, 1..=180);
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
                    changed |= setting_control(
                        ui,
                        "Weekly focus days · 1–7",
                        &mut settings.weekly_goal_days,
                        1..=7,
                    );
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label("Active task");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut settings.active_task)
                            .desired_width(280.0)
                            .hint_text("What are you working on?"),
                    );
                    changed |= response.changed();
                });

                ui.add_space(8.0);
                if ui.checkbox(&mut settings.auto_start_next, "Auto-start next session").changed() {
                    changed = true;
                }
                if ui
                    .checkbox(
                        &mut settings.desktop_notifications,
                        "Show Windows notifications when a session changes",
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(&mut settings.completion_sound, "Play a sound when a session changes")
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
                if ui
                    .checkbox(
                        &mut settings.launch_at_startup,
                        "Launch Pomodoro Focus when Windows starts",
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui.checkbox(&mut settings.compact_timer, "Use compact timer window").changed() {
                    changed = true;
                }
                if ui.checkbox(&mut settings.always_on_top, "Keep timer above other windows").changed() {
                    changed = true;
                }

                if changed {
                    settings.pin = state.settings.pin.clone();
                    settings.port = state.settings.port;
                    settings.start_minimized = state.settings.start_minimized;
                    settings.close_to_tray = state.settings.close_to_tray;
                    settings.remote_import = state.settings.remote_import;
                    if let Err(error) = state.update_settings(settings) {
                        self.settings_saved_at = None;
                        state.error_message = Some(error);
                    } else {
                        state.error_message = None;
                        self.settings_saved_at = Some(Instant::now());
                    }
                }
                if self
                    .settings_saved_at
                    .is_some_and(|saved| saved.elapsed() < Duration::from_secs(3))
                {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Saved").color(SUCCESS_GREEN));
                }
            });
    }

    fn show_notes_section(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        ui.add_space(6.0);
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
            let is_editing_this = self.editing_note.as_ref().is_some_and(|e| e.id == row.id);

            egui::Frame::new()
                .fill(SECONDARY_FILL)
                .stroke(Stroke::new(1.0, SURFACE_BORDER))
                .corner_radius(14.0)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    if is_editing_this {
                        if let Some(editing) = self.editing_note.as_mut() {
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
                                    .desired_width(f32::INFINITY)
                                    .id(egui::Id::new(("note-edit-acc", &editing.id))),
                            );

                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("Subject").size(11.0).color(theme::MUTED),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut editing.subject)
                                    .desired_width(f32::INFINITY)
                                    .id(egui::Id::new(("note-edit-subj", &editing.id))),
                            );

                            ui.add_space(8.0);
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
                        }
                    } else {
                        ui.horizontal(|ui| {
                            if let Some(subject) =
                                row.subject.as_deref().filter(|s| !s.trim().is_empty())
                            {
                                ui.label(
                                    egui::RichText::new(subject)
                                        .size(11.0)
                                        .strong()
                                        .color(SUCCESS_GREEN),
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
                                            self.editing_note = Some(EditingNote {
                                                id: row.id.clone(),
                                                accomplishment: row.accomplishment.clone(),
                                                subject: row.subject.clone().unwrap_or_default(),
                                            });
                                        }
                                    });
                                },
                            );
                        });
                        ui.add_space(2.0);
                        ui.label(egui::RichText::new(&row.accomplishment).color(theme::TEXT));
                        if row.duration_seconds > 0 {
                            ui.add_space(2.0);
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
            let subject_opt = if subject.trim().is_empty() { None } else { Some(subject) };
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

    fn show_task_prompt(&mut self, ui: &mut egui::Ui, state: &mut AppState, accent: Color32) {
        if !state.task_prompt_pending {
            return;
        }

        let mut continue_task = false;
        let mut use_new_task = false;
        let mut dismiss = false;
        egui::Frame::new()
            .fill(theme::PANEL)
            .stroke(Stroke::new(1.0, accent.linear_multiply(0.5)))
            .corner_radius(14.0)
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("Break complete").strong().color(accent));
                    ui.label("Continue your task or choose another.");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.task_prompt_text)
                            .desired_width(220.0)
                            .hint_text("What are you working on?"),
                    );
                    if ui.button("Continue").clicked() {
                        continue_task = true;
                    }
                    if ui.button("Use task").clicked() {
                        use_new_task = true;
                    }
                    if ui.button("Dismiss").clicked() {
                        dismiss = true;
                    }
                });
            });

        if continue_task {
            state.task_prompt_pending = false;
            if !state.timer.running
                && let Err(error) = state.execute_command("start")
            {
                state.error_message = Some(error);
            }
        } else if use_new_task {
            match state.set_active_task(self.task_prompt_text.clone()) {
                Ok(()) => {
                    state.task_prompt_pending = false;
                    if !state.timer.running
                        && let Err(error) = state.execute_command("start")
                    {
                        state.error_message = Some(error);
                    }
                }
                Err(error) => state.error_message = Some(error),
            }
        } else if dismiss {
            state.task_prompt_pending = false;
        }
    }

    fn show_setup_dialog(&mut self, ctx: &egui::Context, state: &mut AppState) {
        if state.settings.onboarding_complete {
            return;
        }

        let mut start_focus = false;
        let mut save_for_later = false;
        egui::Window::new("Welcome to Pomodoro Focus")
            .id(Id::new("first-run-setup"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Choose a focus length and an optional daily goal.");
                ui.label("You can change both later. Phone remote setup is under More.");
                ui.add_space(10.0);
                ui.add(
                    egui::Slider::new(&mut self.setup_study_minutes, 1..=180)
                        .text("Focus · minutes"),
                );
                ui.add(
                    egui::Slider::new(&mut self.setup_goal_minutes, 0..=720)
                        .step_by(15.0)
                        .text("Daily goal · minutes (0 = off)"),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Save & start focus").clicked() {
                        start_focus = true;
                    }
                    if ui.button("Use defaults for now").clicked() {
                        save_for_later = true;
                    }
                });
            });

        if !start_focus && !save_for_later {
            return;
        }
        let mut settings = state.settings.clone();
        if start_focus {
            settings.study_minutes = self.setup_study_minutes;
            settings.daily_goal_minutes = self.setup_goal_minutes;
        }
        settings.onboarding_complete = true;
        match state.update_settings(settings) {
            Ok(()) => {
                state.error_message = None;
                if start_focus && let Err(error) = state.execute_command("start") {
                    state.error_message = Some(error);
                }
            }
            Err(error) => state.error_message = Some(error),
        }
    }

    fn show_shortcut_help(&mut self, ctx: &egui::Context) {
        if !self.show_shortcuts {
            return;
        }
        let mut open = self.show_shortcuts;
        egui::Window::new("Keyboard shortcuts")
            .id(Id::new("keyboard-shortcuts"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("Space — start or pause");
                ui.label("R — reset the current session");
                ui.label("S — skip to the next session");
                ui.label("F1 — show or hide this help");
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Shortcuts are paused while a text field is focused.")
                        .size(12.0)
                        .color(theme::MUTED),
                );
            });
        self.show_shortcuts = open;
    }

    fn show_note_popup(&mut self, ctx: &egui::Context, state: &mut AppState) {
        let Some(pending_duration) = state.pending_note_duration else {
            return;
        };
        let mut open = true;
        egui::Window::new("What did you accomplish?")
            .id(Id::new("study-note-popup"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("A quick note makes it easier to see what your focus added up to.");
                ui.add_space(8.0);
                ui.label("Accomplishment");
                ui.add(
                    egui::TextEdit::singleline(&mut self.note_text)
                        .desired_width(280.0)
                        .id(egui::Id::new("popup-acc")),
                );
                ui.add_space(6.0);
                ui.label("Subject tag (optional)");
                ui.add(
                    egui::TextEdit::singleline(&mut self.subject_text)
                        .desired_width(280.0)
                        .id(egui::Id::new("popup-subj"))
                        .hint_text(&state.settings.active_task),
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
        if !open && state.pending_note_duration == Some(pending_duration) {
            state.pending_note_duration = None;
            self.note_text.clear();
            self.subject_text.clear();
        }
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
                    .fill(Color32::from_rgb(38, 52, 42))
                    .stroke(Stroke::new(1.0, SUCCESS_GREEN.linear_multiply(0.45)))
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
        let (Some(action), Some(seconds)) =
            (state.pending_power_action.as_ref().map(|p| p.action), state.pending_power_seconds())
        else {
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
                    .stroke(Stroke::new(1.0, ERROR_ROSE.linear_multiply(0.5)))
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
                ui.label("Paste the full path to an `all.json` file exported from this app.");
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.import_path)
                        .hint_text("C:\\Users\\You\\focusv2\\exports\\2026-10-18-1200\\all.json")
                        .desired_width(f32::INFINITY)
                        .id(egui::Id::new("import-path")),
                );
                ui.add_space(10.0);

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
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "⚠ Replace will erase your current history, notes, and reflections.",
                        )
                        .color(ERROR_ROSE)
                        .size(12.0),
                    );
                }
                ui.add_space(10.0);

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

    fn try_import_preview(&mut self, state: &mut AppState) {
        let path = self.import_path.trim();
        if path.is_empty() {
            state.error_message = Some("Enter a file path first".to_owned());
            return;
        }
        match std::fs::read(path) {
            Ok(bytes) => match import::parse(&bytes) {
                Ok(bundle) => {
                    let preview = import::preview(&bundle, &state.history, &state.notes);
                    state.error_message = None;
                    self.set_status(format!(
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

    fn try_import_apply(&mut self, state: &mut AppState) {
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
        let previous_reflections = state.reflections.clone();

        let mut history = std::mem::take(&mut state.history);
        let mut notes = std::mem::take(&mut state.notes);
        let mut reflections = std::mem::take(&mut state.reflections);
        let report = import::apply_with_reflections(
            bundle,
            &mut history,
            &mut notes,
            &mut reflections,
            self.import_mode,
        );
        state.history = history;
        state.notes = notes;
        state.reflections = reflections;

        if let Err(error) = state.storage.save_history(&state.history) {
            state.history = previous_history;
            state.notes = previous_notes;
            state.reflections = previous_reflections;
            state.error_message = Some(format!("Could not save history: {error}"));
            return;
        }
        if let Err(error) = state.storage.save_notes(&state.notes) {
            state.history = previous_history;
            state.notes = previous_notes;
            state.reflections = previous_reflections;
            let _ = state.storage.save_history(&state.history);
            state.error_message = Some(format!("Could not save notes: {error}"));
            return;
        }
        if let Err(error) = state.storage.save_reflections(&state.reflections) {
            state.history = previous_history;
            state.notes = previous_notes;
            state.reflections = previous_reflections;
            let _ = state.storage.save_history(&state.history);
            let _ = state.storage.save_notes(&state.notes);
            let _ = state.storage.save_reflections(&state.reflections);
            state.error_message = Some(format!("Could not save weekly reflections: {error}"));
            return;
        }
        state.current_week_reflection =
            state.reflections.for_week(&crate::storage::WeeklyReflection::current_week_start());
        self.reflection_week_start.clear();

        state.error_message = None;
        self.set_status(format!(
            "Imported: +{} days, {} days updated, +{} notes, {} notes skipped",
            report.history_days_added,
            report.history_days_updated,
            report.notes_added,
            report.notes_skipped,
        ));
        state.ctx.request_repaint();
    }
}

fn set_compact_timer(state: &mut AppState, compact: bool) {
    let mut settings = state.settings.clone();
    settings.compact_timer = compact;
    match state.update_settings(settings) {
        Ok(()) => state.error_message = None,
        Err(error) => state.error_message = Some(error),
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
        changed |=
            ui.add(egui::DragValue::new(value).range(range).speed(0.1).suffix(" min")).changed();
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
        egui::Button::new(egui::RichText::new(label).size(14.5).strong().color(text))
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(24.0)
            .min_size(Vec2::new(width, 48.0)),
    )
}

fn secondary_action_button(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(13.0).color(theme::TEXT))
            .fill(SECONDARY_FILL)
            .stroke(Stroke::new(1.0, SECONDARY_BORDER))
            .corner_radius(24.0)
            .min_size(Vec2::new(width, 48.0)),
    )
}

fn show_qr_code(ui: &mut egui::Ui, address: &str, cache: &mut Option<(String, usize, Vec<bool>)>) {
    if cache.as_ref().is_none_or(|(cached_address, _, _)| cached_address != address) {
        *cache = match qrcode::QrCode::new(address.as_bytes()) {
            Ok(code) => {
                let width = code.width();
                let dark_modules = code
                    .to_colors()
                    .into_iter()
                    .map(|color| color == qrcode::Color::Dark)
                    .collect();
                Some((address.to_owned(), width, dark_modules))
            }
            Err(error) => {
                ui.label(format!("Could not create QR code: {error}"));
                return;
            }
        };
    }
    let Some((_, width, dark_modules)) = cache.as_ref() else {
        return;
    };
    let side = 156.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, Color32::WHITE);

    let module_size = side / (*width as f32 + 8.0);
    let quiet_zone = module_size * 4.0;
    for y in 0..*width {
        for x in 0..*width {
            if dark_modules[y * *width + x] {
                let min = rect.min
                    + Vec2::new(
                        quiet_zone + x as f32 * module_size,
                        quiet_zone + y as f32 * module_size,
                    );
                painter.rect_filled(
                    egui::Rect::from_min_size(min, Vec2::splat(module_size)),
                    0.0,
                    Color32::BLACK,
                );
            }
        }
    }
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
    if hours > 0 { format!("{hours}h {minutes:02}m") } else { format!("{minutes}m") }
}

fn local_note_date(note: &crate::storage::StudyNote) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(&note.timestamp)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Local).date_naive())
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
        .with_min_inner_size([380.0, 520.0])
        .with_title("Pomodoro Focus")
        .with_visible(!settings.start_minimized);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    eframe::NativeOptions { viewport, ..Default::default() }
}

pub fn create_app(ctx: &eframe::CreationContext<'_>) -> Result<PomodoroApp, getrandom::Error> {
    PomodoroApp::new(ctx)
}