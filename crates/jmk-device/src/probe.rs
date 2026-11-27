//! Wiring detection for the setup wizard.
//!
//! Only the pins the user offered are touched, with weak pulls, and one pin at
//! a time driven low against pull-ups. No pin is ever driven high, so two
//! probed pins can never fight each other.
//!
//! A pass over all pins is spread over several calls, one small step each, so
//! that probing many pins cannot hold up the rest of the firmware.

use heapless::Vec;
use jmk_proto::{Contact, Pin, MAX_CONTACTS, MAX_PROBE_PINS};

use crate::pins::{Pins, Pull};

pub(crate) struct Prober {
    candidates: Vec<Pin, MAX_PROBE_PINS>,
    step: usize,
    /// Pins found tied to supply or ground in this pass.
    fixed: u64,
    found: Vec<Contact, MAX_CONTACTS>,
}

impl Prober {
    pub fn new(candidates: Vec<Pin, MAX_PROBE_PINS>) -> Self {
        Self { candidates, step: 0, fixed: 0, found: Vec::new() }
    }

    pub fn candidates(&self) -> &[Pin] {
        &self.candidates
    }

    /// Do the next step. Returns every connection present once a pass over
    /// all candidates is complete.
    pub fn step(&mut self, pins: &mut impl Pins) -> Option<Vec<Contact, MAX_CONTACTS>> {
        let step = self.step;
        self.step += 1;
        match step {
            0 => {
                self.fixed = 0;
                self.found.clear();
                self.pull_all(pins, Pull::Down);
                pins.settle();
                self.note_fixed(pins, true);
                // The pull-ups get until the next step to raise the lines.
                self.pull_all(pins, Pull::Up);
            }
            1 => self.note_fixed(pins, false),
            _ => {
                // Look for grounded pins on every step, not once per pass:
                // a turning knob closes its contacts only for a moment.
                self.note_fixed(pins, false);
                let driven = self.candidates[step - 2];
                if self.is_free(driven) {
                    pins.drive_low(driven);
                    pins.settle();
                    let mut low: Vec<Pin, MAX_PROBE_PINS> = Vec::new();
                    for sensed in &self.candidates {
                        if *sensed != driven && self.is_free(*sensed) && !pins.read(*sensed) {
                            let _ = low.push(*sensed);
                        }
                    }
                    pins.input(driven, Pull::Up);
                    if !low.is_empty() {
                        // A pin that stays low after the driven pin is let
                        // go was pulled down by something else (a knob
                        // contact closing just now), not through a switch.
                        pins.settle();
                        for sensed in low {
                            if pins.read(sensed) {
                                self.note_link(driven, sensed);
                                // Try the other direction now rather than
                                // when its turn comes: a switch without a
                                // diode pressed between the two turns would
                                // show one way in this pass and look like a
                                // switch with a diode.
                                pins.drive_low(sensed);
                                pins.settle();
                                let back = !pins.read(driven);
                                pins.input(sensed, Pull::Up);
                                pins.settle();
                                if back {
                                    self.note_link(sensed, driven);
                                }
                            } else {
                                self.fixed |= 1 << sensed;
                                let _ = self.found.push(Contact::Ground(sensed));
                            }
                        }
                    }
                }
            }
        }
        if self.step < self.candidates.len() + 2 {
            return None;
        }
        self.step = 0;
        Some(self.found.clone())
    }

    fn note_link(&mut self, driven: Pin, sensed: Pin) {
        let link = Contact::Link { driven, sensed };
        if !self.found.contains(&link) {
            let _ = self.found.push(link);
        }
    }

    fn is_free(&self, pin: Pin) -> bool {
        self.fixed & (1 << pin) == 0
    }

    fn pull_all(&self, pins: &mut impl Pins, pull: Pull) {
        for pin in &self.candidates {
            pins.input(*pin, pull);
        }
    }

    /// Record pins that read `level` against the opposite pull.
    fn note_fixed(&mut self, pins: &mut impl Pins, level: bool) {
        for pin in &self.candidates {
            if self.is_free(*pin) && pins.read(*pin) == level {
                self.fixed |= 1 << pin;
                let contact = if level { Contact::Supply(*pin) } else { Contact::Ground(*pin) };
                let _ = self.found.push(contact);
            }
        }
    }
}
