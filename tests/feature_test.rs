use pomodoro_focus::breaks;
use pomodoro_focus::storage::Settings;
use pomodoro_focus::timer::{Mode, Timer};

#[test]
fn break_suggestion_is_none_for_study() {
    assert_eq!(breaks::current_suggestion(Mode::Study, 0), None);
}

#[test]
fn break_suggestion_rotates() {
    let a = breaks::current_suggestion(Mode::ShortBreak, 0).unwrap();
    let b = breaks::current_suggestion(Mode::ShortBreak, breaks::ROTATE_SECS).unwrap();
    assert_ne!(a, b);
}

#[test]
fn break_suggestion_wraps() {
    let first = breaks::current_suggestion(Mode::ShortBreak, 0).unwrap();
    let wrapped = breaks::current_suggestion(Mode::ShortBreak, 60).unwrap();
    assert_eq!(first, wrapped);
}

#[test]
fn goal_default_is_zero() {
    let settings = Settings::default();
    assert_eq!(settings.daily_goal_minutes, 0);
    assert!(!settings.auto_start_next);
    assert!(!settings.remote_power_control);
    assert!(!settings.start_minimized);
    assert!(!settings.close_to_tray);
}

#[test]
fn auto_start_does_not_start_immediately() {
    let settings = Settings::default();
    let mut timer = Timer::new(&settings);
    timer.start();
    timer.remaining_seconds = 0;
    let _ = timer.update(&settings);
    assert!(!timer.running);
}

#[test]
fn elapsed_in_mode_resets_on_advance() {
    let settings = Settings::default();
    let mut timer = Timer::new(&settings);
    timer.start();
    timer.elapsed_in_mode = 42;
    timer.skip(&settings);
    assert_eq!(timer.elapsed_in_mode, 0);
}

#[test]
fn tray_settings_default_off() {
    let settings = Settings::default();
    assert!(!settings.start_minimized);
    assert!(!settings.close_to_tray);
}