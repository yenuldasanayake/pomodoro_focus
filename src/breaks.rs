//! Break-time suggestions shown in the timer ring during short and long
//! breaks. Rotates every `ROTATE_SECS` seconds based on how long the break
//! has been running.

use crate::timer::Mode;

pub const ROTATE_SECS: u64 = 10;

const SHORT_BREAK_TIPS: &[&str] = &[
    "Stand up and stretch",
    "Look 20 feet away for 20 seconds",
    "Drink some water",
    "Take a few deep breaths",
    "Roll your shoulders",
    "Unclench your jaw",
];

const LONG_BREAK_TIPS: &[&str] = &[
    "Go for a short walk",
    "Make a cup of tea",
    "Step outside for fresh air",
    "Tidy one small thing",
    "Rest your eyes away from screens",
    "Do a quick stretch routine",
];

pub fn current_suggestion(mode: Mode, elapsed_in_break: u64) -> Option<&'static str> {
    let list = match mode {
        Mode::Study => return None,
        Mode::ShortBreak => SHORT_BREAK_TIPS,
        Mode::LongBreak => LONG_BREAK_TIPS,
    };
    if list.is_empty() {
        return None;
    }
    let index = (elapsed_in_break / ROTATE_SECS) as usize % list.len();
    Some(list[index])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn study_mode_has_no_suggestion() {
        assert_eq!(current_suggestion(Mode::Study, 5), None);
    }

    #[test]
    fn short_break_rotates_every_ten_seconds() {
        let a = current_suggestion(Mode::ShortBreak, 0).unwrap();
        let b = current_suggestion(Mode::ShortBreak, ROTATE_SECS).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn rotation_wraps_around() {
        let first = current_suggestion(Mode::ShortBreak, 0).unwrap();
        let wrapped = current_suggestion(
            Mode::ShortBreak,
            ROTATE_SECS * SHORT_BREAK_TIPS.len() as u64,
        )
        .unwrap();
        assert_eq!(first, wrapped);
    }

    #[test]
    fn long_break_has_its_own_list() {
        let short = current_suggestion(Mode::ShortBreak, 0).unwrap();
        let long = current_suggestion(Mode::LongBreak, 0).unwrap();
        assert_ne!(short, long);
    }
}