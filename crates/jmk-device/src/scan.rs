//! Debounced key scanning for the wiring described by a [`HardwareConfig`].

use jmk_proto::{DiodeDirection, EncoderConfig, HardwareConfig, MatrixConfig, Pin, MAX_ENCODERS, MAX_KEYS};

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
    /// Scans left during which a key's state is frozen after a change.
    lockout: [u8; MAX_KEYS],
    encoders: [Encoder; MAX_ENCODERS],
}

impl Scanner {
    pub const fn new() -> Self {
        Self {
            pressed: [false; MAX_KEYS],
            lockout: [0; MAX_KEYS],
            encoders: [Encoder { last: 0, steps: 0 }; MAX_ENCODERS],
        }
    }

    /// Put all scanned pins into their idle state and forget key states.
    pub fn configure(&mut self, hardware: &HardwareConfig, pins: &mut impl Pins) {
        *self = Self::new();
        let pull = match hardware.matrix {
            MatrixConfig::Direct { active_high: true, .. } => Pull::Down,
            _ => Pull::Up,
        };
        for pin in hardware.matrix.pins() {
            pins.input(pin, pull);
        }
        for (state, encoder) in self.encoders.iter_mut().zip(&hardware.encoders) {
            pins.input(encoder.a, Pull::Up);
            pins.input(encoder.b, Pull::Up);
            pins.settle();
            state.last = read_encoder(encoder, pins);
        }
    }

    /// Scan once. Must be called every millisecond; debounce time counts scans.
    /// A change is reported at once, then the key is ignored while it bounces.
    pub fn scan(&mut self, hardware: &HardwareConfig, pins: &mut impl Pins, mut on_change: impl FnMut(Change)) {
        let debounce = hardware.debounce_ms;
        let mut update = |pos: usize, raw: bool| {
            if pos >= MAX_KEYS {
                return;
            }
            if self.lockout[pos] > 0 {
                self.lockout[pos] -= 1;
            } else if raw != self.pressed[pos] {
                self.pressed[pos] = raw;
                self.lockout[pos] = debounce;
                on_change(Change::Key { pos: pos as u16, pressed: raw });
            }
        };
        match &hardware.matrix {
            MatrixConfig::None => {}
            MatrixConfig::Direct { pins: keys, active_high } => {
                for (pos, pin) in keys.iter().enumerate() {
                    update(pos, pins.read(*pin) == *active_high);
                }
            }
            MatrixConfig::Matrix { rows, cols, diodes } => {
                let (driven, sensed): (&[Pin], &[Pin]) = match diodes {
                    DiodeDirection::Col2Row => (rows, cols),
                    DiodeDirection::Row2Col => (cols, rows),
                };
                for (d, drive) in driven.iter().enumerate() {
                    pins.drive_low(*drive);
                    pins.settle();
                    for (s, sense) in sensed.iter().enumerate() {
                        let (row, col) = match diodes {
                            DiodeDirection::Col2Row => (d, s),
                            DiodeDirection::Row2Col => (s, d),
                        };
                        update(row * cols.len() + col, !pins.read(*sense));
                    }
                    pins.input(*drive, Pull::Up);
                }
                // Let the last driven line recover before the next scan reads.
                pins.settle();
            }
        }

        let first = hardware.matrix.key_count();
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

fn read_encoder(encoder: &EncoderConfig, pins: &mut impl Pins) -> u8 {
    (pins.read(encoder.a) as u8) << 1 | pins.read(encoder.b) as u8
}
