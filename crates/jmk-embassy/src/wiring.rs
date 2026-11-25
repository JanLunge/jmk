//! The pins the port runs a driver on: the cable of a split and the chain
//! of lights. They can be changed while the keyboard runs. The main loop
//! tells here what is configured, and the port's task for each waits for it.
//!
//! A task must let go of its old pins as soon as it is woken with a new
//! setting, before it waits for anything else: the main loop hands those
//! pins to the keyboard wiring one pass later.

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::signal::Signal;
use jmk_proto::{LinkConfig, Pin};

pub struct Wiring {
    link: Signal<NoopRawMutex, Option<LinkConfig>>,
    light: Signal<NoopRawMutex, Option<Pin>>,
}

impl Wiring {
    pub const fn new() -> Self {
        Self { link: Signal::new(), light: Signal::new() }
    }

    /// The cable between the halves as it is configured from now on. The
    /// first call returns once the saved configuration is known.
    pub async fn link(&self) -> Option<LinkConfig> {
        self.link.wait().await
    }

    /// The data pin of the lights as it is configured from now on.
    pub async fn light(&self) -> Option<Pin> {
        self.light.wait().await
    }

    pub(crate) fn set_link(&self, link: Option<LinkConfig>) {
        self.link.signal(link);
    }

    pub(crate) fn set_light(&self, pin: Option<Pin>) {
        self.light.signal(pin);
    }
}

impl Default for Wiring {
    fn default() -> Self {
        Self::new()
    }
}
