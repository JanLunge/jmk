//! USB HID keyboard usage IDs (usage page 0x07) and modifier bits.

pub const A: u8 = 0x04;
pub const B: u8 = 0x05;
pub const C: u8 = 0x06;
pub const D: u8 = 0x07;
pub const E: u8 = 0x08;
pub const F: u8 = 0x09;
pub const J: u8 = 0x0D;
pub const K: u8 = 0x0E;
pub const Z: u8 = 0x1D;
pub const N1: u8 = 0x1E;
pub const N0: u8 = 0x27;
pub const ENTER: u8 = 0x28;
pub const ESCAPE: u8 = 0x29;
pub const BACKSPACE: u8 = 0x2A;
pub const TAB: u8 = 0x2B;
pub const SPACE: u8 = 0x2C;
pub const F1: u8 = 0x3A;

pub const LEFT_CTRL: u8 = 0xE0;
pub const LEFT_SHIFT: u8 = 0xE1;
pub const LEFT_ALT: u8 = 0xE2;
pub const LEFT_GUI: u8 = 0xE3;
pub const RIGHT_CTRL: u8 = 0xE4;
pub const RIGHT_SHIFT: u8 = 0xE5;
pub const RIGHT_ALT: u8 = 0xE6;
pub const RIGHT_GUI: u8 = 0xE7;

/// Modifier bits as they appear in the first byte of a keyboard report.
pub mod mods {
    pub const LCTRL: u8 = 1 << 0;
    pub const LSHIFT: u8 = 1 << 1;
    pub const LALT: u8 = 1 << 2;
    pub const LGUI: u8 = 1 << 3;
    pub const RCTRL: u8 = 1 << 4;
    pub const RSHIFT: u8 = 1 << 5;
    pub const RALT: u8 = 1 << 6;
    pub const RGUI: u8 = 1 << 7;
}

/// Returns the report modifier bit if `key` is one of the eight modifier usages.
pub const fn modifier_bit(key: u8) -> Option<u8> {
    if key >= LEFT_CTRL && key <= RIGHT_GUI {
        Some(1 << (key - LEFT_CTRL))
    } else {
        None
    }
}
