use heapless::Vec;
use jmk_core::{Action, Combo, HoldTapFlavor, HoldTapProfile, Keymap};
use jmk_proto::{HardwareConfig, MatrixConfig, MAX_COMBOS, MAX_HOLD_TAP_PROFILES, MAX_KEYS, MAX_LAYERS};

/// Everything the user can configure. Lives in RAM and is saved to flash on
/// request.
pub struct Config {
    pub hardware: HardwareConfig,
    pub layer_count: u8,
    pub actions: [[Action; MAX_KEYS]; MAX_LAYERS],
    pub profiles: [HoldTapProfile; MAX_HOLD_TAP_PROFILES],
    pub combos: Vec<Combo, MAX_COMBOS>,
}

impl Config {
    const EMPTY_HARDWARE: HardwareConfig =
        HardwareConfig { matrix: MatrixConfig::None, reserved_pins: Vec::new(), debounce_ms: 5 };
    const DEFAULT_PROFILE: HoldTapProfile =
        HoldTapProfile { flavor: HoldTapFlavor::Balanced, tapping_term_ms: 200, require_prior_idle_ms: 0 };

    /// An unconfigured keyboard: no pins in use, one empty layer.
    pub const fn new() -> Self {
        Self {
            hardware: Self::EMPTY_HARDWARE,
            layer_count: 1,
            actions: [[Action::Transparent; MAX_KEYS]; MAX_LAYERS],
            profiles: [Self::DEFAULT_PROFILE; MAX_HOLD_TAP_PROFILES],
            combos: Vec::new(),
        }
    }

    /// Return to the unconfigured state without building a copy on the stack.
    pub fn reset(&mut self) {
        let Config { hardware, layer_count, actions, profiles, combos } = self;
        *hardware = Self::EMPTY_HARDWARE;
        *layer_count = 1;
        actions.iter_mut().flatten().for_each(|a| *a = Action::Transparent);
        *profiles = [Self::DEFAULT_PROFILE; MAX_HOLD_TAP_PROFILES];
        combos.clear();
    }
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
}
