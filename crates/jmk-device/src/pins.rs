use jmk_proto::Pin;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pull {
    None,
    Up,
    Down,
}

/// Run-time access to GPIOs by number. The only way this crate ever drives a
/// pin is low, and only pins the hardware configuration names.
pub trait Pins {
    /// Bit n is set if GPIO n may be used for keyboard wiring.
    fn usable(&self) -> u64;
    fn input(&mut self, pin: Pin, pull: Pull);
    fn drive_low(&mut self, pin: Pin);
    /// True if the pin reads high.
    fn read(&mut self, pin: Pin) -> bool;
    /// Wait for levels to settle after a pin changed (a few microseconds).
    fn settle(&mut self);
}

pub(crate) fn is_usable(pins: &impl Pins, pin: Pin) -> bool {
    pin < 64 && pins.usable() & (1 << pin) != 0
}
