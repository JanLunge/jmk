//! Battery level for the Bluetooth battery service.
//!
//! How the cell is measured depends on the board: VDDH directly, or a
//! divider on an analog pin that may need a second pin to switch it on.
//! That belongs in the hardware configuration, which has no field for it
//! yet. Until it does, this is the one place the level comes from.

/// The level reported while no measurement is configured.
const UNMEASURED_PERCENT: u8 = 100;

/// Charge in percent. Called when a host connects and once a minute after.
pub fn level_percent() -> u8 {
    UNMEASURED_PERCENT
}
