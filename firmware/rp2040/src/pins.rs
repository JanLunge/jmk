use embassy_rp::gpio::{AnyPin, Flex, Pull as RpPull};
use embassy_rp::pac;
use embassy_time::{block_for, Duration};
use jmk_device::{Pins, Pull};
use jmk_proto::{LinkConfig, Pin};

#[cfg(not(feature = "rp235xb"))]
pub const GPIO_COUNT: usize = 30;
#[cfg(feature = "rp235xb")]
pub const GPIO_COUNT: usize = 48;

/// Hands every GPIO of the chip to the macro named, as `number NAME` pairs:
/// the drivers want pin types where the configuration has numbers.
#[cfg(not(feature = "rp235xb"))]
macro_rules! each_gpio {
    ($then:ident) => {
        $then!(
            0 PIN_0, 1 PIN_1, 2 PIN_2, 3 PIN_3, 4 PIN_4, 5 PIN_5, 6 PIN_6, 7 PIN_7, 8 PIN_8, 9 PIN_9,
            10 PIN_10, 11 PIN_11, 12 PIN_12, 13 PIN_13, 14 PIN_14, 15 PIN_15, 16 PIN_16, 17 PIN_17, 18 PIN_18,
            19 PIN_19, 20 PIN_20, 21 PIN_21, 22 PIN_22, 23 PIN_23, 24 PIN_24, 25 PIN_25, 26 PIN_26, 27 PIN_27,
            28 PIN_28, 29 PIN_29
        )
    };
}
#[cfg(feature = "rp235xb")]
macro_rules! each_gpio {
    ($then:ident) => {
        $then!(
            0 PIN_0, 1 PIN_1, 2 PIN_2, 3 PIN_3, 4 PIN_4, 5 PIN_5, 6 PIN_6, 7 PIN_7, 8 PIN_8, 9 PIN_9,
            10 PIN_10, 11 PIN_11, 12 PIN_12, 13 PIN_13, 14 PIN_14, 15 PIN_15, 16 PIN_16, 17 PIN_17, 18 PIN_18,
            19 PIN_19, 20 PIN_20, 21 PIN_21, 22 PIN_22, 23 PIN_23, 24 PIN_24, 25 PIN_25, 26 PIN_26, 27 PIN_27,
            28 PIN_28, 29 PIN_29, 30 PIN_30, 31 PIN_31, 32 PIN_32, 33 PIN_33, 34 PIN_34, 35 PIN_35, 36 PIN_36,
            37 PIN_37, 38 PIN_38, 39 PIN_39, 40 PIN_40, 41 PIN_41, 42 PIN_42, 43 PIN_43, 44 PIN_44, 45 PIN_45,
            46 PIN_46, 47 PIN_47
        )
    };
}
pub(crate) use each_gpio;

/// GPIOs addressed by number. A pin is only claimed, and its pad only
/// reconfigured, the first time the keyboard logic asks for it.
pub struct RpPins {
    pins: [Option<Flex<'static>>; GPIO_COUNT],
}

impl RpPins {
    /// Must be created only once, and no other code may use the GPIOs.
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

    fn drive_high(&mut self, pin: Pin) {
        if let Some(flex) = self.pin(pin) {
            flex.set_high();
            flex.set_as_output();
        }
    }

    fn read(&mut self, pin: Pin) -> bool {
        self.pin(pin).is_some_and(|flex| flex.is_high())
    }

    fn supports_link(&self, link: &LinkConfig) -> bool {
        crate::link::supported(link)
    }

    fn supports_lights(&self, pin: Pin) -> bool {
        (pin as usize) < GPIO_COUNT
    }

    fn reclaim(&mut self) {
        // A pin that was claimed here once and then lent to the UART or the
        // PIO comes back connected to nothing; a pin never claimed is
        // connected when it is first asked for.
        const NOTHING: u8 = pac::io::vals::Gpio0ctrlFuncsel::NULL as u8;
        #[cfg(not(feature = "rp235x"))]
        const GPIO: u8 = pac::io::vals::Gpio0ctrlFuncsel::SIO_0 as u8;
        #[cfg(feature = "rp235x")]
        const GPIO: u8 = pac::io::vals::Gpio0ctrlFuncsel::SIOB_PROC_0 as u8;
        for (number, _) in self.pins.iter().enumerate().filter(|(_, pin)| pin.is_some()) {
            let ctrl = pac::IO_BANK0.gpio(number).ctrl();
            if ctrl.read().funcsel() == NOTHING {
                ctrl.write(|w| w.set_funcsel(GPIO));
            }
        }
    }

    fn show_lights(&mut self, colors: &[[u8; 3]]) {
        crate::lights::show(colors);
    }

    fn settle(&mut self) {
        // Internal pull-ups are weak (50 to 80 kOhm), so a released line
        // needs tens of microseconds to rise against the wiring capacitance.
        block_for(Duration::from_micros(30));
    }
}
