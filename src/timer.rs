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
    last_tick: Instant,
    fractional_elapsed: Duration,
    study_seconds_since_update: u64,
    /// Snapshot of the settings the current session was started with.
    /// Used by `set_settings` to compute how much time has elapsed even
    /// if the duration for the current mode just changed.
    settings_snapshot: Settings,
}

impl Timer {
    pub fn new(settings: &Settings) -> Self {
        Self {
            mode: Mode::Study,
            remaining_seconds: Mode::Study.duration_seconds(settings),
            completed_study_sessions: 0,
            running: false,
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

    /// Apply new settings.
    ///
    /// If the timer is running mid-session, we preserve the elapsed time
    /// instead of resetting to the new full duration. E.g. if you're 10
    /// minutes into a 25-minute focus and change the duration to 30, you
    /// keep your 10 minutes and now have 20 to go.
    ///
    /// If the timer is paused or the session hasn't started, the timer
    /// is reset to the new full duration as before.
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
            if self.mode == Mode::Study {
                self.study_seconds_since_update += applied;
            }
            self.remaining_seconds -= applied;
        }
        self.last_tick = now;
    }

    fn advance(&mut self, settings: &Settings) {
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
    }

    #[test]
    fn changing_duration_mid_session_preserves_elapsed_time() {
        let settings_25 = Settings {
            study_minutes: 25,
            ..Settings::default()
        };
        let mut timer = Timer::new(&settings_25);
        timer.start();
        // Simulate 10 minutes elapsed.
        timer.remaining_seconds = 15 * 60;

        let settings_30 = Settings {
            study_minutes: 30,
            ..Settings::default()
        };
        timer.set_settings(&settings_30);

        // 10 minutes elapsed, 20 remaining of 30.
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
        // 24 minutes elapsed, 1 remaining.
        timer.remaining_seconds = 60;

        let settings_1 = Settings {
            study_minutes: 1,
            ..Settings::default()
        };
        timer.set_settings(&settings_1);

        // Clamped to at least 1 second so `update` has something to consume.
        assert_eq!(timer.remaining_seconds, 1);
    }
}