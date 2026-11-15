use embassy_rp::gpio::{AnyPin, Flex, Pull as RpPull};
use embassy_time::{block_for, Duration};
use jmk_device::{Pins, Pull};
use jmk_proto::Pin;

const GPIO_COUNT: usize = 30;

/// GPIOs addressed by number. A pin is only claimed, and its pad only
/// reconfigured, the first time the keyboard logic asks for it.
pub struct RpPins {
    pins: [Option<Flex<'static>>; GPIO_COUNT],
}

impl RpPins {
    /// Must be created only once, and no other code may use GPIO 0..=29.
    pub fn new() -> Self {
        Self { pins: [const { None }; GPIO_COUNT] }
    }

    fn pin(&mut self, pin: Pin) -> Option<&mut Flex<'static>> {
        let slot = self.pins.get_mut(pin as usize)?;
        // SAFETY: `RpPins` is the sole owner of all GPIOs (see `new`), and
        // each number is stolen at most once because the slot is then filled.
        Some(slot.get_or_insert_with(|| Flex::new(unsafe { AnyPin::steal(pin) })))
    }
}

impl Pins for RpPins {
    fn usable(&self) -> u64 {
        (1 << GPIO_COUNT) - 1
    }

    fn input(&mut self, pin: Pin, pull: Pull) {
        if let Some(flex) = self.pin(pin) {
            flex.set_as_input();
            flex.set_pull(match pull {
                Pull::None => RpPull::None,
                Pull::Up => RpPull::Up,
                Pull::Down => RpPull::Down,
            });
        }
    }

    fn drive_low(&mut self, pin: Pin) {
        if let Some(flex) = self.pin(pin) {
            flex.set_low();
            flex.set_as_output();
        }
    }

    fn read(&mut self, pin: Pin) -> bool {
        self.pin(pin).is_some_and(|flex| flex.is_high())
    }

    fn settle(&mut self) {
        // Internal pull-ups are weak (50 to 80 kOhm), so a released line
        // needs tens of microseconds to rise against the wiring capacitance.
        block_for(Duration::from_micros(30));
    }
}
