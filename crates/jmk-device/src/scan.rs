//! Debounced key scanning for the wiring described by a [`HardwareConfig`].

use jmk_proto::{DiodeDirection, EncoderConfig, HardwareConfig, MatrixConfig, Pin, ScanConfig, MAX_ENCODERS, MAX_KEYS};

use crate::pins::{Pins, Pull};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Key { pos: u16, pressed: bool },
    /// A position that is pressed and released at once (an encoder click).
    Tap(u16),
}

#[derive(Clone, Copy)]
struct Encoder {
    /// Last reading of the two signals, A in bit 1 and B in bit 0.
    last: u8,
    /// Signal changes since the last click; positive is clockwise.
    steps: i8,
}

pub(crate) struct Scanner {
    pressed: [bool; MAX_KEYS],
    /// Scans left during which a key stays down after its press, whatever it reads.
    lockout: [u8; MAX_KEYS],
    /// Scans in a row a held key has read open.
    open: [u8; MAX_KEYS],
    /// Positions that read a switch a second time and are never reported.
    skip: [bool; MAX_KEYS],
    encoders: [Encoder; MAX_ENCODERS],
    /// Bit n set: position n reads closed at rest and open while pressed.
    inverted: u128,
    /// Wait after each change of a matrix line, 0 for the port's usual.
    settle_us: u16,
    /// Hold the driven lines high between reads.
    drive_high: bool,
}

impl Scanner {
    pub const fn new() -> Self {
        Self {
            pressed: [false; MAX_KEYS],
            lockout: [0; MAX_KEYS],
            open: [0; MAX_KEYS],
            skip: [false; MAX_KEYS],
            encoders: [Encoder { last: 0, steps: 0 }; MAX_ENCODERS],
            inverted: 0,
            settle_us: 0,
            drive_high: false,
        }
    }

    /// Put all scanned pins into their idle state and forget key states.
    pub fn configure(&mut self, hardware: &HardwareConfig, more: &[MatrixConfig], doubles: &[u16], scan: &ScanConfig, pins: &mut impl Pins) {
        *self = Self::new();
        for pos in doubles {
            if let Some(skip) = self.skip.get_mut(*pos as usize) {
                *skip = true;
            }
        }
        self.inverted = scan.inverted.iter().filter(|pos| (**pos as usize) < MAX_KEYS).fold(0, |bits, pos| bits | 1 << pos);
        self.settle_us = scan.settle_us;
        self.drive_high = scan.drive_idle_high;
        idle_pins(hardware, more, pins);
        pins.settle();
        for (state, encoder) in self.encoders.iter_mut().zip(&hardware.encoders) {
            state.last = read_encoder(encoder, pins);
        }
    }

    /// Scan once. Must be called every millisecond; debounce time counts scans.
    /// A press is reported at once and the key is then ignored while it bounces.
    /// A release counts only once the key has read open for the whole debounce
    /// time, so neither a bouncing release nor a contact that lets go for a
    /// moment while held types the key a second time.
    pub fn scan(
        &mut self,
        hardware: &HardwareConfig,
        more: &[MatrixConfig],
        pins: &mut impl Pins,
        mut on_change: impl FnMut(Change),
    ) {
        let debounce = hardware.debounce_ms;
        let (inverted, settle_us) = (self.inverted, self.settle_us);
        let lines = Lines { settle_us, drive_high: self.drive_high };
        let mut update = |pos: usize, raw: bool| {
            if pos >= MAX_KEYS || self.skip[pos] {
                return;
            }
            let raw = raw != (inverted >> pos & 1 != 0);
            if !self.pressed[pos] {
                if raw {
                    self.pressed[pos] = true;
                    self.lockout[pos] = debounce;
                    self.open[pos] = 0;
                    on_change(Change::Key { pos: pos as u16, pressed: true });
                }
            } else if self.lockout[pos] > 0 {
                self.lockout[pos] -= 1;
            } else if raw {
                self.open[pos] = 0;
            } else if self.open[pos] >= debounce {
                self.pressed[pos] = false;
                on_change(Change::Key { pos: pos as u16, pressed: false });
            } else {
                self.open[pos] += 1;
            }
        };
        // Each group of switches takes the positions after the group before it.
        let mut base = 0;
        for part in hardware.wiring(more) {
            match part {
                MatrixConfig::None => {}
                MatrixConfig::Direct { pins: keys, active_high } => {
                    for (index, pin) in keys.iter().enumerate() {
                        update(base + index, pins.read(*pin) == *active_high);
                    }
                }
                MatrixConfig::Matrix { rows, cols, diodes } => {
                    let crossings = rows.len() * cols.len();
                    // Lines held high could not be read in a matrix read both ways.
                    let lines = Lines { drive_high: lines.drive_high && *diodes != DiodeDirection::Both, ..lines };
                    if *diodes != DiodeDirection::Row2Col {
                        lines.scan(rows, cols, pins, |row, col, down| update(base + row * cols.len() + col, down));
                    }
                    if *diodes != DiodeDirection::Col2Row {
                        // The second switch of each crossing in a duplex matrix comes after all the first ones.
                        let start = if *diodes == DiodeDirection::Both { base + crossings } else { base };
                        lines.scan(cols, rows, pins, |col, row, down| update(start + row * cols.len() + col, down));
                    }
                }
            }
            base += part.key_count();
        }

        let first = base;
        for (index, (state, encoder)) in self.encoders.iter_mut().zip(&hardware.encoders).enumerate() {
            // Quadrature decoding: which way each valid signal change turns.
            const TURN: [i8; 16] = [0, -1, 1, 0, 1, 0, 0, -1, -1, 0, 0, 1, 0, 1, -1, 0];
            let now = read_encoder(encoder, pins);
            state.steps += TURN[(state.last << 2 | now) as usize];
            state.last = now;
            let detent = encoder.steps_per_detent.clamp(1, 16) as i8;
            if state.steps.abs() >= detent {
                let clockwise = state.steps > 0;
                state.steps = 0;
                on_change(Change::Tap((first + 2 * index + clockwise as usize) as u16));
            }
        }
    }
}

/// How matrix lines are driven, from the [`ScanConfig`].
#[derive(Clone, Copy)]
struct Lines {
    /// Wait after each change of a line, 0 for the port's usual.
    settle_us: u16,
    /// Hold lines high between reads instead of letting them go.
    drive_high: bool,
}

impl Lines {
    /// Pull each of `driven` low in turn and report which of `sensed` follow.
    ///
    /// A line pulled low may also switch on power for sensor keys (the
    /// Svalboard powers one finger cluster at a time through a transistor on
    /// each row). `settle_us` then gives them time to wake before the read,
    /// which is also the time the line before has to go quiet, and
    /// `drive_high` switches the others off properly. The wait comes once per
    /// line: a scan must stay well inside its millisecond.
    fn scan(self, driven: &[Pin], sensed: &[Pin], pins: &mut impl Pins, mut found: impl FnMut(usize, usize, bool)) {
        if self.drive_high {
            // Again on every scan: a pin change elsewhere may have let them go.
            for line in driven {
                pins.drive_high(*line);
            }
        }
        for (d, drive) in driven.iter().enumerate() {
            pins.drive_low(*drive);
            self.settle(pins);
            for (s, sense) in sensed.iter().enumerate() {
                found(d, s, !pins.read(*sense));
            }
            if self.drive_high {
                pins.drive_high(*drive);
            } else {
                pins.input(*drive, Pull::Up);
            }
        }
        // Let the last driven line recover before pins of another group are read.
        pins.settle();
    }

    fn settle(self, pins: &mut impl Pins) {
        match self.settle_us {
            0 => pins.settle(),
            us => pins.wait_us(us),
        }
    }
}

/// Put all scanned pins into their idle state, leaving key states alone.
pub fn idle_pins(hardware: &HardwareConfig, more: &[MatrixConfig], pins: &mut impl Pins) {
    for part in hardware.wiring(more) {
        let pull = match part {
            MatrixConfig::Direct { active_high: true, .. } => Pull::Down,
            _ => Pull::Up,
        };
        for pin in part.pins() {
            pins.input(pin, pull);
        }
    }
    for encoder in &hardware.encoders {
        pins.input(encoder.a, Pull::Up);
        pins.input(encoder.b, Pull::Up);
    }
}

fn read_encoder(encoder: &EncoderConfig, pins: &mut impl Pins) -> u8 {
    (pins.read(encoder.a) as u8) << 1 | pins.read(encoder.b) as u8
}
