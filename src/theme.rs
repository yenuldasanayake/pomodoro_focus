use crate::timer::Mode;
use eframe::egui::{self, Color32, Stroke};

pub const BACKGROUND: Color32 = Color32::from_rgb(32, 33, 36);   // Google Dark Surface (#202124)
pub const PANEL: Color32 = Color32::from_rgb(41, 42, 45);        // Elevated Surface
pub const TEXT: Color32 = Color32::from_rgb(232, 234, 237);       // Primary Text (#e8eaed)
pub const MUTED: Color32 = Color32::from_rgb(154, 160, 166);      // Secondary Text (#9aa0a6)

pub fn accent(mode: Mode) -> Color32 {
    match mode {
        Mode::Study => Color32::from_rgb(138, 180, 248),      // Google Blue (#8ab4f8)
        Mode::ShortBreak => Color32::from_rgb(129, 201, 149), // Google Green (#81c995)
        Mode::LongBreak => Color32::from_rgb(242, 139, 130),  // Google Red (#f28b82)
    }
}

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.dark_mode = true;
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = Color32::from_rgb(20, 21, 23);
    visuals.faint_bg_color = PANEL;
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(95, 99, 104));
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(60, 64, 67);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(80, 86, 92);
    visuals.widgets.active.bg_fill = Color32::from_rgb(138, 180, 248);
    ctx.set_visuals(visuals);
}