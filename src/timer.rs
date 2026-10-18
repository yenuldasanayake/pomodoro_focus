use crate::storage::Settings;
use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Study,
    ShortBreak,
    LongBreak,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Study => "FOCUS SESSION",
            Self::ShortBreak => "SHORT BREAK",
            Self::LongBreak => "LONG BREAK",
        }
    }

    pub fn duration_seconds(self, settings: &Settings) -> u64 {
        let minutes = match self {
            Self::Study => settings.study_minutes,
            Self::ShortBreak => settings.short_break_minutes,
            Self::LongBreak => settings.long_break_minutes,
        };
        u64::from(minutes) * 60
    }
}

#[derive(Debug)]
pub struct Timer {
    pub mode: Mode,
    pub remaining_seconds: u64,
    pub completed_study_sessions: u32,
    pub running: bool,
    pub elapsed_in_mode: u64,
    last_tick: Instant,
    fractional_elapsed: Duration,
    study_seconds_since_update: u64,
    settings_snapshot: Settings,
}

impl Timer {
    pub fn new(settings: &Settings) -> Self {
        Self {
            mode: Mode::Study,
            remaining_seconds: Mode::Study.duration_seconds(settings),
            completed_study_sessions: 0,
            running: false,
            elapsed_in_mode: 0,
            last_tick: Instant::now(),
            fractional_elapsed: Duration::ZERO,
            study_seconds_since_update: 0,
            settings_snapshot: settings.clone(),
        }
    }

    pub fn start(&mut self) {
        self.running = true;
        self.last_tick = Instant::now();
    }

    pub fn pause(&mut self) {
        self.tick_elapsed();
        self.running = false;
    }

    pub fn reset(&mut self, settings: &Settings) {
        self.running = false;
        self.remaining_seconds = self.mode.duration_seconds(settings);
        self.elapsed_in_mode = 0;
        self.last_tick = Instant::now();
        self.settings_snapshot = settings.clone();
    }

    pub fn skip(&mut self, settings: &Settings) {
        self.running = false;
        self.advance(settings);
    }

    pub fn update(&mut self, settings: &Settings) -> Option<Mode> {
        if !self.running {
            return None;
        }
        self.tick_elapsed();
        if self.remaining_seconds > 0 {
            return None;
        }

        let completed = self.mode;
        if completed == Mode::Study {
            self.completed_study_sessions += 1;
        }
        self.advance(settings);
        Some(completed)
    }

    pub fn progress(&self, settings: &Settings) -> f32 {
        let total = self.mode.duration_seconds(settings).max(1);
        (1.0 - self.remaining_seconds as f32 / total as f32).clamp(0.0, 1.0)
    }

    pub fn set_settings(&mut self, settings: &Settings) {
        let old_total = self.mode.duration_seconds(&self.settings_snapshot);
        let new_total = self.mode.duration_seconds(settings);

        if self.running && self.remaining_seconds > 0 && old_total > 0 {
            let elapsed = old_total.saturating_sub(self.remaining_seconds);
            self.remaining_seconds = new_total.saturating_sub(elapsed).max(1);
        } else {
            self.remaining_seconds = new_total;
        }

        self.last_tick = Instant::now();
        self.fractional_elapsed = Duration::ZERO;
        self.settings_snapshot = settings.clone();
    }

    pub fn take_study_seconds(&mut self) -> u64 {
        std::mem::take(&mut self.study_seconds_since_update)
    }

    fn tick_elapsed(&mut self) {
        let now = Instant::now();
        if self.running {
            let elapsed = now.duration_since(self.last_tick) + self.fractional_elapsed;
            let elapsed_seconds = elapsed.as_secs();
            self.fractional_elapsed = elapsed - Duration::from_secs(elapsed_seconds);
            let applied = elapsed_seconds.min(self.remaining_seconds);
            self.elapsed_in_mode += applied;
            if self.mode == Mode::Study {
                self.study_seconds_since_update += applied;
            }
            self.remaining_seconds -= applied;
        }
        self.last_tick = now;
    }

    pub fn advance(&mut self, settings: &Settings) {
        self.mode = match self.mode {
            Mode::Study => {
                let every = settings.sessions_before_long_break.max(1);
                if self.completed_study_sessions > 0
                    && self.completed_study_sessions.is_multiple_of(every)
                {
                    Mode::LongBreak
                } else {
                    Mode::ShortBreak
                }
            }
            Mode::ShortBreak | Mode::LongBreak => Mode::Study,
        };
        self.remaining_seconds = self.mode.duration_seconds(settings);
        self.elapsed_in_mode = 0;
        self.last_tick = Instant::now();
        self.fractional_elapsed = Duration::ZERO;
        self.settings_snapshot = settings.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn study_sessions_schedule_long_break_at_configured_interval() {
        let settings = Settings {
            sessions_before_long_break: 2,
            ..Settings::default()
        };
        let mut timer = Timer::new(&settings);
        timer.start();

        timer.remaining_seconds = 0;
        assert_eq!(timer.update(&settings), Some(Mode::Study));
        assert_eq!(timer.mode, Mode::ShortBreak);

        timer.remaining_seconds = 0;
        assert_eq!(timer.update(&settings), Some(Mode::ShortBreak));
        assert_eq!(timer.mode, Mode::Study);

        timer.remaining_seconds = 0;
        assert_eq!(timer.update(&settings), Some(Mode::Study));
        assert_eq!(timer.mode, Mode::LongBreak);
    }

    #[test]
    fn reset_stops_timer_and_restores_current_mode_duration() {
        let settings = Settings::default();
        let mut timer = Timer::new(&settings);
        timer.mode = Mode::LongBreak;
        timer.remaining_seconds = 1;
        timer.start();

        timer.reset(&settings);

        assert!(!timer.running);
        assert_eq!(timer.remaining_seconds, 15 * 60);
        assert_eq!(timer.elapsed_in_mode, 0);
    }

    #[test]
    fn changing_duration_mid_session_preserves_elapsed_time() {
        let settings_25 = Settings {
            study_minutes: 25,
            ..Settings::default()
        };
        let mut timer = Timer::new(&settings_25);
        timer.start();
        timer.remaining_seconds = 15 * 60;

        let settings_30 = Settings {
            study_minutes: 30,
            ..Settings::default()
        };
        timer.set_settings(&settings_30);

        assert_eq!(timer.remaining_seconds, 20 * 60);
        assert!(timer.running);
    }

    #[test]
    fn changing_duration_while_paused_resets_to_new_total() {
        let settings_25 = Settings {
            study_minutes: 25,
            ..Settings::default()
        };
        let mut timer = Timer::new(&settings_25);
        timer.remaining_seconds = 15 * 60;

        let settings_30 = Settings {
            study_minutes: 30,
            ..Settings::default()
        };
        timer.set_settings(&settings_30);

        assert_eq!(timer.remaining_seconds, 30 * 60);
        assert!(!timer.running);
    }

    #[test]
    fn shortening_below_elapsed_keeps_at_least_one_second() {
        let settings_25 = Settings {
            study_minutes: 25,
            ..Settings::default()
        };
        let mut timer = Timer::new(&settings_25);
        timer.start();
        timer.remaining_seconds = 60;

        let settings_1 = Settings {
            study_minutes: 1,
            ..Settings::default()
        };
        timer.set_settings(&settings_1);

        assert_eq!(timer.remaining_seconds, 1);
    }
}