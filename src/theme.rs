use crate::timer::Mode;
use eframe::egui::{self, Color32, Stroke};

pub const BACKGROUND: Color32 = Color32::from_rgb(24, 24, 37);
pub const PANEL: Color32 = Color32::from_rgb(30, 30, 46);
pub const TEXT: Color32 = Color32::from_rgb(230, 230, 239);
pub const MUTED: Color32 = Color32::from_rgb(145, 146, 166);

pub fn accent(mode: Mode) -> Color32 {
    match mode {
        Mode::Study => Color32::from_rgb(235, 118, 128),
        Mode::ShortBreak => Color32::from_rgb(166, 227, 161),
        Mode::LongBreak => Color32::from_rgb(137, 180, 250),
    }
}

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.dark_mode = true;
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = Color32::from_rgb(17, 17, 27);
    visuals.faint_bg_color = PANEL;
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(49, 50, 68));
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(49, 50, 68);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(69, 71, 90);
    visuals.widgets.active.bg_fill = Color32::from_rgb(89, 91, 110);
    ctx.set_visuals(visuals);
}
