//! Keyboard logic shared by every microcontroller port: matrix scanning,
//! wiring detection, configuration storage and the configurator protocol.
//!
//! A port supplies a [`Pins`] implementation, a flash driver and transports;
//! everything here also runs on a PC against simulated hardware.
#![cfg_attr(not(test), no_std)]

mod config;
mod device;
mod lights;
mod link;
mod pins;
mod probe;
mod scan;
mod storage;
mod sync;

pub use config::Config;
pub use device::{Board, Device};
pub use link::Link;
pub use pins::{Pins, Pull};
pub use storage::{Storage, STORAGE_SIZE};
