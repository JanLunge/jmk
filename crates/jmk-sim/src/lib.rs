//! Simulated keyboard hardware that runs the real device logic.
//!
//! Used by the tests and, compiled to WebAssembly, by the configurator's
//! demo keyboards. Nothing here depends on threads or the system clock.

mod hardware;
mod keyboard;

pub use hardware::{MemFlash, Pipe, SimPins, Wire};
pub use keyboard::{HostEvent, VirtualKeyboard, Wiring};
