use embassy_nrf::gpio::{AnyPin, Flex, OutputDrive, Pull as NrfPull};
use jmk_device::{Pins, Pull};
use jmk_proto::{LinkConfig, Pin};

/// P0.00 to P0.31 are pins 0 to 31, P1.00 to P1.15 are pins 32 to 47.
const GPIO_COUNT: usize = 48;

/// Pins that keyboard wiring must never touch on any nRF52840 board:
/// P0.00 and P0.01 are the 32 kHz crystal pins and P0.18 is the reset pin
/// (embassy-nrf configures it as such in UICR). P0.09 and P0.10, the NFC
/// antenna pins, are usable: the `nfc-pins-as-gpio` feature of embassy-nrf
/// turns them into GPIOs, as keyboard controller boards wire them to the
/// edge like any other pin.
const NEVER_USABLE: u64 = 1 << 0 | 1 << 1 | 1 << 18;

/// GPIOs addressed by number. A pin is only claimed, and its pad only
/// reconfigured, the first time the keyboard logic asks for it.
pub struct NrfPins {
    pins: [Option<Flex<'static>>; GPIO_COUNT],
}

impl NrfPins {
    /// Must be created only once, and no other code may use a GPIO unless
    /// the hardware configuration gave it that pin (the split cable).
    pub fn new() -> Self {
        Self { pins: [const { None }; GPIO_COUNT] }
    }

    fn pin(&mut self, pin: Pin) -> Option<&mut Flex<'static>> {
        if NEVER_USABLE & (1 << pin) != 0 {
            return None;
        }
        let slot = self.pins.get_mut(pin as usize)?;
        // SAFETY: `NrfPins` is the sole owner of all GPIOs (see `new`), and
        // each number is stolen at most once because the slot is then filled.
        // The number is embassy-nrf's own: port * 32 + pin.
        Some(slot.get_or_insert_with(|| Flex::new(unsafe { AnyPin::steal(pin) })))
    }
}

impl Pins for NrfPins {
    fn usable(&self) -> u64 {
        ((1 << GPIO_COUNT) - 1) & !NEVER_USABLE
    }

    fn input(&mut self, pin: Pin, pull: Pull) {
        if let Some(flex) = self.pin(pin) {
            flex.set_as_input(match pull {
                Pull::None => NrfPull::None,
                Pull::Up => NrfPull::Up,
                Pull::Down => NrfPull::Down,
            });
        }
    }

    fn drive_low(&mut self, pin: Pin) {
        if let Some(flex) = self.pin(pin) {
            flex.set_low();
            flex.set_as_output(OutputDrive::Standard);
        }
    }

    fn read(&mut self, pin: Pin) -> bool {
        self.pin(pin).is_some_and(|flex| flex.is_high())
    }

    fn supports_link(&self, link: &LinkConfig) -> bool {
        crate::link::supported(link)
    }

    fn settle(&mut self) {
        // The internal pull resistors are about 13 kOhm (nRF52840 product
        // specification, GPIO electrical specification), so a released line
        // rises within a few microseconds. The time driver ticks at
        // 32768 Hz, too coarse for this, so count CPU cycles: 64 per
        // microsecond.
        cortex_m::asm::delay(64 * 10);
    }
}
