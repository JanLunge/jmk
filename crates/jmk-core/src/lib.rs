//! Hardware-independent keymap engine.
//!
//! The engine turns timestamped key press/release events into HID output.
//! It knows nothing about pins, USB or Bluetooth, so it runs unchanged on a
//! microcontroller and in host tests.
#![cfg_attr(not(test), no_std)]

pub mod action;
mod combo;
mod engine;
pub mod keycode;

pub use action::{Action, Basic, Combo, ComboOptions, Direction, HoldTapFlavor, HoldTapProfile, MacroStep, MouseKeys, MAX_COMBO_KEYS};
pub use engine::{Engine, KeyboardReport, Keymap, MouseReport, Output, Resolved, MAX_REPORT_KEYS, MAX_RESOLVED};

/// Milliseconds since boot.
pub type Millis = u64;

/// Where an event came from: a physical key position or a fired combo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Key(u16),
    Combo(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Event {
    pub src: Source,
    pub pressed: bool,
    pub time: Millis,
}
