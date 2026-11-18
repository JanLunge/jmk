//! Data model and wire protocol shared by the firmware and the configurator.
//!
//! Messages are serialised with postcard and split into fixed-size HID
//! reports by [`frame`].
#![cfg_attr(not(test), no_std)]

pub mod frame;

pub use heapless::{String, Vec};
use jmk_core::{Action, Combo, HoldTapProfile, MacroStep};
use serde::{Deserialize, Serialize};

/// 2 added the layer events and [`Request::HoldLow`]; 3 added lighting; 4 layer colours.
pub const PROTOCOL_VERSION: u16 = 4;

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
pub const MAX_ENCODERS: usize = 4;
pub const MAX_MACROS: usize = 16;
/// Steps shared by all macros.
pub const MAX_MACRO_STEPS: usize = 512;
pub const MACRO_STEPS_PER_CHUNK: usize = 32;
pub const LAYOUT_KEYS_PER_CHUNK: usize = 16;
pub const MAX_NAME: usize = 32;
/// Lights on one chain.
pub const MAX_LEDS: usize = 120;
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

/// A rotary encoder. Each one adds two positions after the switches:
/// counter-clockwise, then clockwise. A detent acts as a tap on that position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncoderConfig {
    pub a: Pin,
    pub b: Pin,
    /// Signal changes per click, usually 4 (sometimes 2).
    pub steps_per_detent: u8,
}

/// How two halves talk to each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkConfig {
    /// Serial cable, each side's `tx` wired to the other's `rx`.
    Uart { tx: Pin, rx: Pin },
    /// Bluetooth between the halves (chips with a radio only).
    Ble,
}

/// The part this board plays in a split keyboard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitConfig {
    /// A keyboard in one piece.
    #[default]
    None,
    /// Talks to the computer and holds the keymap. Positions of the other
    /// half follow this half's own positions.
    Central(LinkConfig),
    /// Only reports its keys to the central half.
    Peripheral(LinkConfig),
}

impl SplitConfig {
    pub fn link(&self) -> Option<LinkConfig> {
        match self {
            SplitConfig::None => None,
            SplitConfig::Central(link) | SplitConfig::Peripheral(link) => Some(*link),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareConfig {
    pub matrix: MatrixConfig,
    /// Pins wired to displays, LEDs, power circuits and the like. Scanning
    /// and wiring detection never touch them.
    pub reserved_pins: Vec<Pin, MAX_RESERVED_PINS>,
    pub debounce_ms: u8,
    pub encoders: Vec<EncoderConfig, MAX_ENCODERS>,
    pub split: SplitConfig,
}

impl HardwareConfig {
    /// Positions this board produces itself: switches, then two per encoder.
    pub fn local_positions(&self) -> usize {
        self.matrix.key_count() + 2 * self.encoders.len()
    }

    /// Every pin the configuration puts to use.
    pub fn used_pins(&self) -> impl Iterator<Item = Pin> + '_ {
        let link = match self.split.link() {
            Some(LinkConfig::Uart { tx, rx }) => [Some(tx), Some(rx)],
            _ => [None, None],
        };
        self.matrix
            .pins()
            .chain(self.encoders.iter().flat_map(|e| [e.a, e.b]))
            .chain(link.into_iter().flatten())
    }
}

/// The order in which a light chain expects its three colours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorOrder {
    /// WS2812 and SK6812, the usual keyboard lights.
    #[default]
    Grb,
    Rgb,
}

/// One light on the chain, in chain order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Led {
    /// The key position this light sits under, or [`Led::UNDERGLOW`] for a
    /// light that shines out of the bottom of the case.
    pub key: u8,
    /// Where it sits, with 0..=255 spanning the longer side of the keyboard.
    pub x: u8,
    pub y: u8,
}

impl Led {
    pub const UNDERGLOW: u8 = 255;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    #[default]
    Off,
    /// One colour.
    Solid,
    /// One colour fading in and out.
    Breathing,
    /// All colours moving across the keyboard.
    Rainbow,
    /// Dark until a key is pressed: that key's light flashes and fades. The
    /// underglow flashes on every key.
    Reactive,
}

/// What one group of lights shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightZone {
    pub effect: Effect,
    /// Colour as hue, saturation and brightness, each 0..=255.
    pub hue: u8,
    pub sat: u8,
    pub val: u8,
    pub speed: u8,
}

/// A chain of addressable RGB lights (WS2812 or compatible) on one pin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightingConfig {
    /// The chain's data pin. The one pin the firmware drives high; a changed
    /// pin takes effect after a restart.
    pub pin: Option<Pin>,
    pub order: ColorOrder,
    /// Every colour is scaled by this over 255, so the lights cannot draw
    /// more current than the USB port gives.
    pub limit: u8,
    pub leds: Vec<Led, MAX_LEDS>,
    /// The lights under keys.
    pub keys: LightZone,
    pub underglow: LightZone,
}

impl LightingConfig {
    pub const NONE: Self = Self {
        pin: None,
        order: ColorOrder::Grb,
        limit: 80,
        leds: Vec::new(),
        keys: LightZone { effect: Effect::Off, hue: 0, sat: 0, val: 0, speed: 0 },
        underglow: LightZone { effect: Effect::Off, hue: 0, sat: 0, val: 0, speed: 0 },
    };
}

impl Default for LightingConfig {
    fn default() -> Self {
        Self::NONE
    }
}

/// Where a key sits on the keyboard, for drawing it. Units are hundredths of
/// a standard key width; rotation is in tenths of a degree around the key's
/// centre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyGeometry {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
    pub r: i16,
}

/// Descriptive data the firmware stores but does not interpret.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    pub name: String<MAX_NAME>,
    /// Identifier of the controller board in the configurator's catalogue.
    pub controller: String<MAX_NAME>,
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
    /// Unique per chip, so the configurator can tell two halves apart.
    pub uid: [u8; 8],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitStatus {
    /// The other half was heard from recently.
    pub connected: bool,
    /// Positions the other half reports (central only).
    pub remote_positions: u16,
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
    // The wire format numbers these in order. `GetInfo` and `Reboot` must
    // keep their places for ever, so any configurator can identify and
    // update any firmware; add new requests at the end.
    GetInfo,
    Reboot { bootloader: bool },
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
    /// Read up to [`MACRO_STEPS_PER_CHUNK`] steps of a macro from `start`.
    GetMacro { index: u8, start: u16 },
    /// Replace a macro's steps from `start` to its end. Send the first chunk
    /// with `start` 0 and continue with `start` at the length so far.
    SetMacro { index: u8, start: u16, steps: Vec<MacroStep, MACRO_STEPS_PER_CHUNK> },
    GetMeta,
    SetMeta(Meta),
    /// Read up to [`LAYOUT_KEYS_PER_CHUNK`] key shapes from `start`.
    GetLayout { start: u16 },
    /// Replace the stored key shapes from `start` to the end.
    SetLayout { start: u16, keys: Vec<KeyGeometry, LAYOUT_KEYS_PER_CHUNK> },
    GetSplitStatus,
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
    /// Hold one pin low (`Some`) or let it go again (`None`). While a pin is
    /// held the keyboard does not scan its keys. Used to find a wire to
    /// another board, which sees the pin as grounded in its own wiring
    /// detection. The caller must have ruled out pins tied to supply voltage,
    /// e.g. with [`Request::StartProbe`] on this board first.
    HoldLow(Option<Pin>),
    GetLighting,
    /// Replace the lighting configuration. Colours and effects change at
    /// once; the pin after a restart.
    SetLighting(LightingConfig),
    /// Light one light of the chain on its own (`Some`), so the user can see
    /// where it sits, or return to the configured effects (`None`).
    IdentifyLed(Option<u8>),
    GetLayerColors,
    /// One colour number per layer, for whoever draws the keymap. The
    /// keyboard only stores them; 0 means none was chosen.
    SetLayerColors(Vec<u8, MAX_LAYERS>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    // `Ok`, `Error` and `Info` keep their places; add new replies at the end.
    Ok,
    Error(Error),
    Info(Info),
    Hardware(HardwareConfig),
    LayerCount(u8),
    Actions(Vec<Action, ACTIONS_PER_CHUNK>),
    HoldTapProfiles(Vec<HoldTapProfile, MAX_HOLD_TAP_PROFILES>),
    Combos(Vec<Combo, MAX_COMBOS>),
    /// `total` is the macro's full length.
    Macro { total: u16, steps: Vec<MacroStep, MACRO_STEPS_PER_CHUNK> },
    Meta(Meta),
    /// `total` is the number of stored key shapes.
    Layout { total: u16, keys: Vec<KeyGeometry, LAYOUT_KEYS_PER_CHUNK> },
    SplitStatus(SplitStatus),
    Lighting(LightingConfig),
    LayerColors(Vec<u8, MAX_LAYERS>),
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
    /// No room left, e.g. for macro steps.
    Full,
    /// The chip cannot do this, e.g. Bluetooth on a chip without a radio or
    /// a serial link on pins that have no serial function.
    Unsupported,
    Storage,
}

/// Sent by the device without a request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    Key { pos: u16, pressed: bool },
    /// All contacts currently seen, sent whenever the set changes.
    Probe(Vec<Contact, MAX_CONTACTS>),
    /// The pressed key at `pos` took its action from `layer`. Follows the
    /// key's press event, later if a hold-tap had to be decided first.
    Resolved { pos: u16, layer: u8 },
    /// Bitmask of the layers now active. Sent when key watching starts and
    /// whenever it changes.
    Layers(u32),
}

/// Everything the device sends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceMessage {
    Response(Response),
    Event(Event),
}
