//! What every Embassy-based port shares: the three USB HID interfaces, the
//! queues between the keyboard logic and the transports, the main loop and
//! the pump for the split cable.
//!
//! A port adds the chip: GPIO by number, flash, the USB driver, the UART and
//! how to reset.
#![cfg_attr(not(test), no_std)]

pub mod link;
pub mod queue;
pub mod run;
pub mod usb;
pub mod wiring;
