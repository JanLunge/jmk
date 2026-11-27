use heapless::{String, Vec};
use jmk_core::{Action, Combo, HoldTapFlavor, HoldTapProfile, Keymap, MacroStep};
use jmk_proto::{
    Doubles, HardwareConfig, KeyGeometry, LetterRows, LightingConfig, MoreWiring, MatrixConfig, Meta, SplitConfig, MAX_COMBOS, MAX_HOLD_TAP_PROFILES, MAX_KEYS,
    MAX_LAYERS, MAX_MACROS, MAX_MACRO_NAME, MAX_MACRO_STEPS,
};

pub type MacroName = String<MAX_MACRO_NAME>;

/// Everything the user can configure. Lives in RAM and is saved to flash on
/// request.
pub struct Config {
    pub hardware: HardwareConfig,
    pub layer_count: u8,
    pub actions: [[Action; MAX_KEYS]; MAX_LAYERS],
    pub profiles: [HoldTapProfile; MAX_HOLD_TAP_PROFILES],
    pub combos: Vec<Combo, MAX_COMBOS>,
    pub meta: Meta,
    pub layout: Vec<KeyGeometry, MAX_KEYS>,
    /// The steps of all macros, one macro after the other.
    pub macro_steps: Vec<MacroStep, MAX_MACRO_STEPS>,
    /// Where each macro's steps end within `macro_steps`.
    pub macro_ends: [u16; MAX_MACROS],
    pub lighting: LightingConfig,
    /// A colour number per layer, kept for the configurator.
    pub layer_colors: [u8; MAX_LAYERS],
    /// What the user calls each macro, kept for the configurator.
    pub macro_names: [MacroName; MAX_MACROS],
    /// Where the letters sit, kept for the configurator.
    pub letter_rows: LetterRows,
    /// Groups of switches besides `hardware.matrix`.
    pub more_wiring: MoreWiring,
    /// Positions that read a switch a second time and are not reported.
    pub doubles: Doubles,
    /// How many times the shared part was saved with a change in it; see
    /// [`crate::sync`].
    pub revision: u32,
    /// [`Config::shared_digest`] when the two halves last agreed.
    pub base: Option<u32>,
}

/// The part both halves of a split hold alike: the keyboard's name, keymap,
/// behaviours, macros, key shapes and layer colours. The wiring, the lights
/// and the controller's name are each half's own.
struct Shared<'a>(&'a Config);

impl serde::Serialize for Shared<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        let config = self.0;
        let layers = &config.actions[..config.layer_count as usize];
        let mut parts = serializer.serialize_tuple(8 + layers.len())?;
        parts.serialize_element(&config.layer_count)?;
        parts.serialize_element(&config.profiles)?;
        parts.serialize_element(&config.combos)?;
        parts.serialize_element(&config.layout)?;
        parts.serialize_element(&config.macro_ends)?;
        parts.serialize_element(&config.macro_steps)?;
        parts.serialize_element(&config.layer_colors)?;
        parts.serialize_element(&config.meta.name)?;
        for layer in layers {
            parts.serialize_element(&layer[..])?;
        }
        // Only when there are any, so that a keymap without named macros reads
        // the same as on firmware from before macros had names.
        let rows = config.letter_rows.iter().any(|row| !row.is_empty());
        if rows || config.macro_names.iter().any(|name| !name.is_empty()) {
            parts.serialize_element(&config.macro_names)?;
        }
        // Likewise the letter rows, which came after the names.
        if rows {
            parts.serialize_element(&config.letter_rows)?;
        }
        parts.end()
    }
}

/// Keeps the bytes from `skip` on, as many as fit, and counts all of them.
struct Window<'a> {
    skip: usize,
    out: &'a mut [u8],
    filled: usize,
    total: usize,
}

impl postcard::ser_flavors::Flavor for Window<'_> {
    type Output = (usize, usize);
    fn try_push(&mut self, byte: u8) -> postcard::Result<()> {
        if self.total >= self.skip && self.filled < self.out.len() {
            self.out[self.filled] = byte;
            self.filled += 1;
        }
        self.total += 1;
        Ok(())
    }
    fn finalize(self) -> postcard::Result<(usize, usize)> {
        Ok((self.filled, self.total))
    }
}

impl Config {
    const EMPTY_HARDWARE: HardwareConfig = HardwareConfig {
        matrix: MatrixConfig::None,
        reserved_pins: Vec::new(),
        debounce_ms: 5,
        encoders: Vec::new(),
        split: SplitConfig::None,
    };
    const DEFAULT_PROFILE: HoldTapProfile =
        HoldTapProfile { flavor: HoldTapFlavor::Balanced, tapping_term_ms: 200, require_prior_idle_ms: 0 };
    const NO_NAME: MacroName = String::new();
    const NO_ROWS: LetterRows = [Vec::new(), Vec::new(), Vec::new()];
    const EMPTY_META: Meta = Meta { name: String::new(), controller: String::new() };

    /// An unconfigured keyboard: no pins in use, one empty layer.
    pub const fn new() -> Self {
        Self {
            hardware: Self::EMPTY_HARDWARE,
            layer_count: 1,
            actions: [[Action::Transparent; MAX_KEYS]; MAX_LAYERS],
            profiles: [Self::DEFAULT_PROFILE; MAX_HOLD_TAP_PROFILES],
            combos: Vec::new(),
            meta: Self::EMPTY_META,
            layout: Vec::new(),
            macro_steps: Vec::new(),
            macro_ends: [0; MAX_MACROS],
            lighting: LightingConfig::NONE,
            layer_colors: [0; MAX_LAYERS],
            macro_names: [Self::NO_NAME; MAX_MACROS],
            letter_rows: Self::NO_ROWS,
            more_wiring: Vec::new(),
            doubles: Vec::new(),
            revision: 0,
            base: None,
        }
    }

    /// Return to the unconfigured state without building a copy on the stack.
    pub fn reset(&mut self) {
        let Config {
            hardware,
            layer_count,
            actions,
            profiles,
            combos,
            meta,
            layout,
            macro_steps,
            macro_ends,
            lighting,
            layer_colors,
            macro_names,
            letter_rows,
            more_wiring,
            doubles,
            revision,
            base,
        } = self;
        *hardware = Self::EMPTY_HARDWARE;
        *layer_count = 1;
        actions.iter_mut().flatten().for_each(|a| *a = Action::Transparent);
        *profiles = [Self::DEFAULT_PROFILE; MAX_HOLD_TAP_PROFILES];
        combos.clear();
        *meta = Self::EMPTY_META;
        layout.clear();
        macro_steps.clear();
        *macro_ends = [0; MAX_MACROS];
        *lighting = LightingConfig::NONE;
        *layer_colors = [0; MAX_LAYERS];
        macro_names.iter_mut().for_each(|name| name.clear());
        *letter_rows = Self::NO_ROWS;
        more_wiring.clear();
        doubles.clear();
        *revision = 0;
        *base = None;
    }

    /// A number that is the same on two halves exactly when they hold the
    /// same shared part.
    pub fn shared_digest(&self) -> u32 {
        struct Fnv(u32);
        impl postcard::ser_flavors::Flavor for Fnv {
            type Output = u32;
            fn try_push(&mut self, byte: u8) -> postcard::Result<()> {
                self.0 = (self.0 ^ byte as u32).wrapping_mul(0x0100_0193);
                Ok(())
            }
            fn finalize(self) -> postcard::Result<u32> {
                Ok(self.0)
            }
        }
        postcard::serialize_with_flavor(&Shared(self), Fnv(0x811C_9DC5)).unwrap_or(0)
    }

    /// Copy the shared part's bytes from `offset` on into `out`. Returns how
    /// many were copied and how long the whole is.
    pub(crate) fn shared_chunk(&self, offset: usize, out: &mut [u8]) -> (usize, usize) {
        let window = Window { skip: offset, out, filled: 0, total: 0 };
        postcard::serialize_with_flavor(&Shared(self), window).unwrap_or((0, 0))
    }

    /// Replace the shared part with what [`Config::shared_chunk`] produced on
    /// the other half. On failure it may be half replaced.
    pub(crate) fn set_shared(&mut self, mut bytes: &[u8]) -> Option<()> {
        fn take<T: serde::de::DeserializeOwned>(bytes: &mut &[u8]) -> Option<T> {
            let (value, rest) = postcard::take_from_bytes(bytes).ok()?;
            *bytes = rest;
            Some(value)
        }
        let layer_count: u8 = take(&mut bytes)?;
        if layer_count == 0 || layer_count as usize > MAX_LAYERS {
            return None;
        }
        self.layer_count = layer_count;
        self.profiles = take(&mut bytes)?;
        self.combos = take(&mut bytes)?;
        self.layout = take(&mut bytes)?;
        self.macro_ends = take(&mut bytes)?;
        self.macro_steps = take(&mut bytes)?;
        self.layer_colors = take(&mut bytes)?;
        self.meta.name = take(&mut bytes)?;
        for layer in &mut self.actions[..layer_count as usize] {
            let actions: Vec<Action, MAX_KEYS> = take(&mut bytes)?;
            if actions.len() != MAX_KEYS {
                return None;
            }
            layer.copy_from_slice(&actions);
        }
        if bytes.is_empty() {
            self.macro_names.iter_mut().for_each(|name| name.clear());
        } else {
            self.macro_names = take(&mut bytes)?;
        }
        self.letter_rows = if bytes.is_empty() { Self::NO_ROWS } else { take(&mut bytes)? };
        bytes.is_empty().then_some(())
    }

    fn macro_range(&self, index: usize) -> Option<core::ops::Range<usize>> {
        let end = *self.macro_ends.get(index)? as usize;
        let start = if index == 0 { 0 } else { self.macro_ends[index - 1] as usize };
        (start <= end && end <= self.macro_steps.len()).then_some(start..end)
    }

    /// Replace macro `index` from its step `start` to its end with `steps`.
    /// Returns false if `start` is past its end or there is no room.
    pub fn set_macro(&mut self, index: usize, start: usize, steps: &[MacroStep]) -> Result<(), MacroError> {
        let range = self.macro_range(index).ok_or(MacroError::OutOfRange)?;
        if start > range.len() {
            return Err(MacroError::OutOfRange);
        }
        let from = range.start + start;
        let (removed, added) = (range.end - from, steps.len());
        let old_len = self.macro_steps.len();
        let new_len = old_len - removed + added;
        if new_len > MAX_MACRO_STEPS {
            return Err(MacroError::Full);
        }
        // Move the macros behind this one, then drop the new steps in.
        if added > removed {
            let _ = self.macro_steps.resize(new_len, MacroStep::DelayMs(0));
        }
        self.macro_steps.copy_within(range.end..old_len, from + added);
        self.macro_steps.truncate(new_len);
        self.macro_steps[from..from + added].copy_from_slice(steps);
        for end in &mut self.macro_ends[index..] {
            *end = (*end as usize - removed + added) as u16;
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum MacroError {
    OutOfRange,
    Full,
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

impl Keymap for Config {
    fn layer_count(&self) -> u8 {
        self.layer_count
    }

    fn action(&self, layer: u8, pos: u16) -> Action {
        self.actions
            .get(layer as usize)
            .and_then(|l| l.get(pos as usize))
            .copied()
            .unwrap_or_default()
    }

    fn hold_tap_profile(&self, profile: u8) -> HoldTapProfile {
        self.profiles.get(profile as usize).copied().unwrap_or_default()
    }

    fn combos(&self) -> &[Combo] {
        &self.combos
    }

    fn macro_steps(&self, index: u8) -> &[MacroStep] {
        self.macro_range(index as usize).map_or(&[], |range| &self.macro_steps[range])
    }
}
