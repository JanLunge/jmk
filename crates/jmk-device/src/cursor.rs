//! The mouse: turns held mouse buttons and mouse keys into mouse reports.
//!
//! The engine only says what is held ([`jmk_core::Engine::mouse_keys`]);
//! how far and how often the cursor moves is decided here, once every
//! millisecond. Trackpads and inertia will feed into this stage as well.

use jmk_core::{Direction, Millis, MouseKeys, MouseReport, Output};

/// Time between two steps of a moving cursor.
const MOVE_INTERVAL_MS: Millis = 10;
/// Pixels per step when a direction key is first pressed, and once it has
/// been held for `ACCEL_MS`. The speed grows evenly in between.
const MOVE_START: i32 = 2;
const MOVE_MAX: i32 = 20;
const ACCEL_MS: Millis = 1000;
/// A wheel key turns the wheel one step at once, then repeats like a held
/// key: after `WHEEL_DELAY_MS`, every `WHEEL_INTERVAL_MS`.
const WHEEL_DELAY_MS: Millis = 300;
const WHEEL_INTERVAL_MS: Millis = 50;

const UP: u8 = Direction::Up.bit();
const DOWN: u8 = Direction::Down.bit();
const LEFT: u8 = Direction::Left.bit();
const RIGHT: u8 = Direction::Right.bit();

/// Moving or turning the wheel, which repeats while its keys are held.
#[derive(Clone, Copy, Default)]
struct Repeat {
    /// Since when its keys have been held, and when it next takes a step.
    held: Option<(Millis, Millis)>,
}

impl Repeat {
    /// Whether a step is due now, and since when the keys are held.
    fn due(&mut self, active: bool, now: Millis, next: impl FnOnce(Millis) -> Millis) -> Option<Millis> {
        if !active {
            self.held = None;
            return None;
        }
        let (since, at) = *self.held.get_or_insert((now, now));
        if now < at {
            return None;
        }
        self.held = Some((since, next(now - since) + now));
        Some(since)
    }
}

#[derive(Default)]
pub(crate) struct Cursor {
    /// The buttons the computer was last told about.
    buttons: u8,
    moving: Repeat,
    wheel: Repeat,
}

impl Cursor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Call every millisecond with what the engine holds.
    pub fn tick(&mut self, keys: MouseKeys, now: Millis, out: &mut impl Output) {
        let mut report = MouseReport { buttons: keys.buttons, ..MouseReport::default() };
        if let Some(since) = self.moving.due(keys.moving != 0, now, |_| MOVE_INTERVAL_MS) {
            let speed = speed(now - since);
            report.x = (axis(keys.moving, RIGHT, LEFT) * speed) as i16;
            report.y = (axis(keys.moving, DOWN, UP) * speed) as i16;
        }
        let wheel_wait = |held| if held == 0 { WHEEL_DELAY_MS } else { WHEEL_INTERVAL_MS };
        if self.wheel.due(keys.wheel != 0, now, wheel_wait).is_some() {
            report.wheel = axis(keys.wheel, UP, DOWN) as i8;
            report.pan = axis(keys.wheel, RIGHT, LEFT) as i8;
        }
        let moved = report.x != 0 || report.y != 0 || report.wheel != 0 || report.pan != 0;
        if moved || report.buttons != self.buttons {
            out.mouse(&report);
            self.buttons = report.buttons;
        }
    }
}

/// 1, -1 or 0 (both or neither held).
fn axis(held: u8, plus: u8, minus: u8) -> i32 {
    (held & plus != 0) as i32 - (held & minus != 0) as i32
}

/// Pixels per step after the keys have been held this long.
fn speed(held: Millis) -> i32 {
    let held = held.min(ACCEL_MS) as i32;
    MOVE_START + (MOVE_MAX - MOVE_START) * held / ACCEL_MS as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Log(std::vec::Vec<MouseReport>);

    impl Output for Log {
        fn keyboard(&mut self, _: &jmk_core::KeyboardReport) {}
        fn mouse(&mut self, report: &MouseReport) {
            self.0.push(*report);
        }
    }

    fn run(cursor: &mut Cursor, keys: MouseKeys, from: Millis, to: Millis) -> std::vec::Vec<MouseReport> {
        let mut log = Log::default();
        for now in from..to {
            cursor.tick(keys, now, &mut log);
        }
        log.0
    }

    #[test]
    fn buttons_are_sent_once_per_change() {
        let mut cursor = Cursor::new();
        let left = MouseKeys { buttons: 1, ..Default::default() };
        assert_eq!(run(&mut cursor, left, 0, 5), [MouseReport { buttons: 1, ..Default::default() }]);
        assert_eq!(run(&mut cursor, MouseKeys::default(), 5, 10), [MouseReport::default()]);
        assert!(run(&mut cursor, MouseKeys::default(), 10, 20).is_empty());
    }

    #[test]
    fn cursor_moves_at_once_then_every_interval_and_speeds_up() {
        let mut cursor = Cursor::new();
        let keys = MouseKeys { moving: RIGHT | UP, ..Default::default() };
        let reports = run(&mut cursor, keys, 100, 100 + ACCEL_MS + 1);
        assert_eq!(reports.len() as Millis, ACCEL_MS / MOVE_INTERVAL_MS + 1);
        assert_eq!((reports[0].x, reports[0].y), (MOVE_START as i16, -MOVE_START as i16));
        let last = reports.last().unwrap();
        assert_eq!((last.x, last.y), (MOVE_MAX as i16, -MOVE_MAX as i16));
        assert!(reports.windows(2).all(|w| w[1].x >= w[0].x));
        // Let go and press again: slow again.
        run(&mut cursor, MouseKeys::default(), 2000, 2001);
        assert_eq!(run(&mut cursor, keys, 2001, 2002)[0].x, MOVE_START as i16);
    }

    #[test]
    fn opposite_directions_cancel() {
        let mut cursor = Cursor::new();
        assert!(run(&mut cursor, MouseKeys { moving: LEFT | RIGHT, ..Default::default() }, 0, 50).is_empty());
    }

    #[test]
    fn wheel_steps_once_then_repeats_after_a_delay() {
        let mut cursor = Cursor::new();
        let down = MouseKeys { wheel: DOWN, ..Default::default() };
        let reports = run(&mut cursor, down, 0, WHEEL_DELAY_MS);
        assert_eq!(reports, [MouseReport { wheel: -1, ..Default::default() }]);
        let reports = run(&mut cursor, down, WHEEL_DELAY_MS, WHEEL_DELAY_MS + 2 * WHEEL_INTERVAL_MS);
        assert_eq!(reports.len(), 2);
        let right = MouseKeys { wheel: RIGHT, ..Default::default() };
        assert_eq!(run(&mut cursor, right, 1000, 1001)[0].pan, 1);
    }

    #[test]
    fn moving_and_turning_the_wheel_at_once_are_kept_apart() {
        let mut cursor = Cursor::new();
        let keys = MouseKeys { moving: UP, wheel: UP, ..Default::default() };
        let reports = run(&mut cursor, keys, 0, 1);
        assert_eq!(reports, [MouseReport { y: -MOVE_START as i16, wheel: 1, ..Default::default() }]);
    }

    #[test]
    fn a_button_change_rides_along_with_movement() {
        let mut cursor = Cursor::new();
        let reports = run(&mut cursor, MouseKeys { buttons: 1, moving: DOWN, ..Default::default() }, 0, 1);
        assert_eq!(reports, [MouseReport { buttons: 1, y: MOVE_START as i16, ..Default::default() }]);
    }
}
