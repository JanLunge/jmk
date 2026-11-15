//! Data model and wire protocol shared by the firmware and the configurator.
//!
//! Messages are serialised with postcard and split into fixed-size HID
//! reports by [`frame`].
#![cfg_attr(not(test), no_std)]

pub mod frame;

pub use heapless::Vec;
use jmk_core::{Action, Combo, HoldTapProfile};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

pub const MAX_KEYS: usize = 128;
pub const MAX_LAYERS: usize = 8;
pub const MAX_COMBOS: usize = 16;
pub const MAX_HOLD_TAP_PROFILES: usize = 4;
pub const MAX_MATRIX_ROWS: usize = 16;
pub const MAX_MATRIX_COLS: usize = 24;
pub const MAX_DIRECT_PINS: usize = 64;
pub const MAX_RESERVED_PINS: usize = 32;
pub const MAX_PROBE_PINS: usize = 48;
pub const MAX_CONTACTS: usize = 16;
/// Actions per keymap read/write message.
pub const ACTIONS_PER_CHUNK: usize = 16;
/// Largest serialised message in either direction.
pub const MAX_MESSAGE: usize = 512;

/// A GPIO number in the microcontroller's own numbering
/// (RP2040: GPIO n; nRF52840: P0.n = n, P1.n = 32 + n).
pub type Pin = u8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiodeDirection {
    /// Rows are driven low one at a time, columns are read.
    Col2Row,
    /// Columns are driven low one at a time, rows are read.
    Row2Col,
}

/// How the switches are wired. Key positions are numbered in the order given
/// in each variant's documentation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatrixConfig {
    #[default]
    None,
    /// One pin per switch. Position = index into `pins`.
    Direct {
        pins: Vec<Pin, MAX_DIRECT_PINS>,
        /// Switches connect the pin to supply instead of ground.
        active_high: bool,
    },
    /// Row/column matrix. Position = row * cols.len() + col.
    Matrix {
        rows: Vec<Pin, MAX_MATRIX_ROWS>,
        cols: Vec<Pin, MAX_MATRIX_COLS>,
        diodes: DiodeDirection,
    },
}

impl MatrixConfig {
    pub fn key_count(&self) -> usize {
        match self {
            MatrixConfig::None => 0,
            MatrixConfig::Direct { pins, .. } => pins.len(),
            MatrixConfig::Matrix { rows, cols, .. } => rows.len() * cols.len(),
        }
    }

    pub fn pins(&self) -> impl Iterator<Item = Pin> + '_ {
        let (a, b): (&[Pin], &[Pin]) = match self {
            MatrixConfig::None => (&[], &[]),
            MatrixConfig::Direct { pins, .. } => (pins, &[]),
            MatrixConfig::Matrix { rows, cols, .. } => (rows, cols),
        };
        a.iter().chain(b).copied()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareConfig {
    pub matrix: MatrixConfig,
    /// Pins wired to displays, LEDs, power circuits and the like. Scanning
    /// and wiring detection never touch them.
    pub reserved_pins: Vec<Pin, MAX_RESERVED_PINS>,
    pub debounce_ms: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mcu {
    Rp2040,
    Rp2350,
    Nrf52840,
    /// Host-side simulation.
    Simulated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Info {
    pub protocol: u16,
    pub firmware: (u8, u8, u8),
    pub mcu: Mcu,
    /// Bit n is set if GPIO n may be used for keyboard wiring on this chip.
    pub usable_pins: u64,
    pub max_keys: u16,
    pub max_layers: u8,
}

/// An electrical connection seen during wiring detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Contact {
    /// The pin is connected to ground (a pressed direct-wired switch).
    Ground(Pin),
    /// The pin is connected to supply.
    Supply(Pin),
    /// Pulling `driven` low pulls `sensed` low. A switch without a diode
    /// shows up twice, once in each direction.
    Link { driven: Pin, sensed: Pin },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    GetInfo,
    GetHardware,
    /// Replace the hardware configuration. Takes effect immediately.
    SetHardware(HardwareConfig),
    GetLayerCount,
    SetLayerCount(u8),
    /// Read up to [`ACTIONS_PER_CHUNK`] actions starting at `pos`.
    GetActions { layer: u8, pos: u16 },
    SetActions { layer: u8, pos: u16, actions: Vec<Action, ACTIONS_PER_CHUNK> },
    GetHoldTapProfiles,
    SetHoldTapProfile { index: u8, profile: HoldTapProfile },
    GetCombos,
    SetCombos(Vec<Combo, MAX_COMBOS>),
    /// Write the current configuration to flash.
    Save,
    /// Erase the stored configuration and return to defaults.
    FactoryReset,
    /// Send [`Event::Key`] for every debounced key change.
    WatchKeys(bool),
    /// Stop normal scanning and report connections between these pins.
    /// Reserved and unusable pins are rejected.
    StartProbe(Vec<Pin, MAX_PROBE_PINS>),
    StopProbe,
    Reboot { bootloader: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Ok,
    Error(Error),
    Info(Info),
    Hardware(HardwareConfig),
    LayerCount(u8),
    Actions(Vec<Action, ACTIONS_PER_CHUNK>),
    HoldTapProfiles(Vec<HoldTapProfile, MAX_HOLD_TAP_PROFILES>),
    Combos(Vec<Combo, MAX_COMBOS>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Error {
    /// The request could not be decoded.
    Malformed,
    OutOfRange,
    /// A pin does not exist on this chip or is not allowed for wiring.
    UnusablePin(Pin),
    /// A pin is listed as reserved in the hardware configuration.
    ReservedPin(Pin),
    /// The same pin is used twice.
    DuplicatePin(Pin),
    TooManyKeys,
    Storage,
}

/// Sent by the device without a request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    Key { pos: u16, pressed: bool },
    /// All contacts currently seen, sent whenever the set changes.
    Probe(Vec<Contact, MAX_CONTACTS>),
}

/// Everything the device sends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceMessage {
    Response(Response),
    Event(Event),
}
