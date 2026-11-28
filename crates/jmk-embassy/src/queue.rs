//! Queues between the keyboard logic and a transport to the computer.

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use jmk_core::{KeyboardReport, MouseReport, Output};
use jmk_proto::frame::REPORT_SIZE;

pub type KeyboardQueue = Channel<NoopRawMutex, [u8; 8], 32>;
pub type ConsumerQueue = Channel<NoopRawMutex, u16, 8>;
pub type MouseQueue = Channel<NoopRawMutex, [u8; 7], 16>;
pub type FrameQueue = Channel<NoopRawMutex, [u8; REPORT_SIZE], 16>;

/// Key reports waiting for one transport (USB, Bluetooth).
pub struct HidQueues {
    pub keyboard: KeyboardQueue,
    pub consumer: ConsumerQueue,
    pub mouse: MouseQueue,
}

impl HidQueues {
    pub const fn new() -> Self {
        Self { keyboard: Channel::new(), consumer: Channel::new(), mouse: Channel::new() }
    }

    /// Forget reports nobody collected, e.g. from before a host connected.
    pub fn clear(&self) {
        self.keyboard.clear();
        self.consumer.clear();
        self.mouse.clear();
    }
}

impl Default for HidQueues {
    fn default() -> Self {
        Self::new()
    }
}

impl Output for &HidQueues {
    fn keyboard(&mut self, report: &KeyboardReport) {
        if self.keyboard.try_send(report.boot()).is_err() {
            // The host stopped polling. Reports carry the full state, so
            // only the newest one matters once it listens again.
            self.keyboard.clear();
            let _ = self.keyboard.try_send(report.boot());
        }
    }

    fn consumer(&mut self, usage: u16) {
        if self.consumer.try_send(usage).is_err() {
            self.consumer.clear();
            let _ = self.consumer.try_send(usage);
        }
    }

    fn mouse(&mut self, report: &MouseReport) {
        if self.mouse.try_send(report.bytes()).is_err() {
            // Movement waiting here is lost; the buttons are in every report.
            self.mouse.clear();
            let _ = self.mouse.try_send(report.bytes());
        }
    }
}

/// Configurator frames in both directions.
pub struct ConfigQueues {
    pub to_host: FrameQueue,
    pub from_host: FrameQueue,
}

impl ConfigQueues {
    pub const fn new() -> Self {
        Self { to_host: Channel::new(), from_host: Channel::new() }
    }
}

impl Default for ConfigQueues {
    fn default() -> Self {
        Self::new()
    }
}
