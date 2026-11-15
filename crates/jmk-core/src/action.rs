//! What a key does. These types are stored in flash and sent to the
//! configurator, so they are small, `Copy` and serialisable.

use heapless::Vec;
use serde::{Deserialize, Serialize};

/// An action that needs no timing decision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Basic {
    #[default]
    None,
    /// HID keyboard usage, including the modifier usages 0xE0..=0xE7.
    Key(u8),
    /// One or more modifiers, as a report bitmask.
    Mods(u8),
    /// A key sent together with modifiers, e.g. Shift+1.
    ModKey { mods: u8, key: u8 },
    /// HID consumer usage (media keys).
    Consumer(u16),
    /// Layer is active while the key is held.
    MomentaryLayer(u8),
    ToggleLayer(u8),
    /// Switch to this layer and turn all other toggled layers off.
    ToLayer(u8),
    /// Modifiers that apply to the next key press only.
    OneShotMods(u8),
    /// Play the macro with this index.
    Macro(u8),
    /// Handed to the firmware's plugin hooks via [`crate::Output::custom`].
    Custom(u16),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// Fall through to the next lower active layer.
    #[default]
    Transparent,
    Basic(Basic),
    /// `tap` when tapped, `hold` when held. `profile` selects a [`HoldTapProfile`].
    HoldTap { hold: Basic, tap: Basic, profile: u8 },
}

impl Action {
    pub const NONE: Action = Action::Basic(Basic::None);

    pub const fn key(key: u8) -> Action {
        Action::Basic(Basic::Key(key))
    }
}

/// How an undecided hold-tap reacts to other keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoldTapFlavor {
    /// Hold only once the tapping term has passed.
    TapPreferred,
    /// Hold if another key is pressed and released while this one is down.
    #[default]
    Balanced,
    /// Hold as soon as another key is pressed.
    HoldPreferred,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldTapProfile {
    pub flavor: HoldTapFlavor,
    pub tapping_term_ms: u16,
    /// If another key was pressed less than this long ago, always tap.
    /// Avoids accidental holds while typing quickly. 0 disables it.
    pub require_prior_idle_ms: u16,
}

impl Default for HoldTapProfile {
    fn default() -> Self {
        Self { flavor: HoldTapFlavor::Balanced, tapping_term_ms: 200, require_prior_idle_ms: 0 }
    }
}

pub const MAX_COMBO_KEYS: usize = 4;

/// Keys that trigger an action when pressed together within `timeout_ms`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Combo {
    pub keys: Vec<u16, MAX_COMBO_KEYS>,
    pub action: Action,
    pub timeout_ms: u16,
}

/// One step of a macro. Steps run a couple of milliseconds apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MacroStep {
    /// Press and release a key (HID usage, modifiers included).
    Tap(u8),
    Press(u8),
    Release(u8),
    DelayMs(u16),
}
