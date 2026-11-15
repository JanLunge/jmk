//! Debounced key scanning for the wiring described by a [`MatrixConfig`].

use jmk_proto::{DiodeDirection, MatrixConfig, Pin, MAX_KEYS};

use crate::pins::{Pins, Pull};

pub(crate) struct Scanner {
    pressed: [bool; MAX_KEYS],
    /// Scans left during which a key's state is frozen after a change.
    lockout: [u8; MAX_KEYS],
}

impl Scanner {
    pub const fn new() -> Self {
        Self { pressed: [false; MAX_KEYS], lockout: [0; MAX_KEYS] }
    }

    /// Put all pins of `matrix` into their idle state and forget key states.
    pub fn configure(&mut self, matrix: &MatrixConfig, pins: &mut impl Pins) {
        *self = Self::new();
        let pull = match matrix {
            MatrixConfig::Direct { active_high: true, .. } => Pull::Down,
            _ => Pull::Up,
        };
        for pin in matrix.pins() {
            pins.input(pin, pull);
        }
    }

    /// Scan once. Must be called every millisecond; `debounce` is in scans.
    /// A change is reported at once, then the key is ignored while it bounces.
    pub fn scan(
        &mut self,
        matrix: &MatrixConfig,
        debounce: u8,
        pins: &mut impl Pins,
        mut on_change: impl FnMut(u16, bool),
    ) {
        let mut update = |pos: usize, raw: bool| {
            if pos >= MAX_KEYS {
                return;
            }
            if self.lockout[pos] > 0 {
                self.lockout[pos] -= 1;
            } else if raw != self.pressed[pos] {
                self.pressed[pos] = raw;
                self.lockout[pos] = debounce;
                on_change(pos as u16, raw);
            }
        };
        match matrix {
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
    }
}
