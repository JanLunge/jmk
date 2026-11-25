use jmk_proto::{LinkConfig, Pin};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pull {
    None,
    Up,
    Down,
}

/// Run-time access to GPIOs by number, and to the light chain. The only way
/// this crate ever drives a pin is low, and only pins the hardware
/// configuration names; the light chain's data pin belongs to the port.
pub trait Pins {
    /// Bit n is set if GPIO n may be used for keyboard wiring.
    fn usable(&self) -> u64;
    fn input(&mut self, pin: Pin, pull: Pull);
    fn drive_low(&mut self, pin: Pin);
    /// True if the pin reads high.
    fn read(&mut self, pin: Pin) -> bool;
    /// Whether this chip can run a split link as described. Called before a
    /// hardware configuration is accepted.
    fn supports_link(&self, _link: &LinkConfig) -> bool {
        false
    }
    /// Whether a light chain can be run from this pin.
    fn supports_lights(&self, _pin: Pin) -> bool {
        false
    }
    /// Send colours down the light chain, three bytes per light in the
    /// chain's own colour order. The port sets the chain up at start from
    /// the saved configuration and ignores this if there is none.
    fn show_lights(&mut self, _colors: &[[u8; 3]]) {}
    /// The pin for the lights or the pins for the cable changed, and the
    /// port's drivers have let go of the old ones: make them usable for
    /// wiring again. They are set up as inputs right after.
    fn reclaim(&mut self) {}
    /// Wait for levels to settle after a pin changed (a few microseconds).
    fn settle(&mut self);
}

pub(crate) fn is_usable(pins: &impl Pins, pin: Pin) -> bool {
    pin < 64 && pins.usable() & (1 << pin) != 0
}
