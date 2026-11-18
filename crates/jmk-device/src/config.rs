use heapless::{String, Vec};
use jmk_core::{Action, Combo, HoldTapFlavor, HoldTapProfile, Keymap, MacroStep};
use jmk_proto::{
    HardwareConfig, KeyGeometry, LightingConfig, MatrixConfig, Meta, SplitConfig, MAX_COMBOS, MAX_HOLD_TAP_PROFILES, MAX_KEYS,
    MAX_LAYERS, MAX_MACROS, MAX_MACRO_STEPS,
};

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
        }
    }

    /// Return to the unconfigured state without building a copy on the stack.
    pub fn reset(&mut self) {
        let Config { hardware, layer_count, actions, profiles, combos, meta, layout, macro_steps, macro_ends, lighting, layer_colors } = self;
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
