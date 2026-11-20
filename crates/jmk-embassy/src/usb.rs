//! The USB side every port presents: a boot keyboard, consumer control and
//! the vendor channel for the configurator.

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_futures::join::join;
use embassy_usb::class::hid::{Config as HidConfig, HidBootProtocol, HidReaderWriter, HidSubclass, HidWriter, State};
use embassy_usb::driver::Driver;
use embassy_usb::{Builder, Config, Handler, UsbDevice};
use jmk_proto::frame::REPORT_SIZE;
use static_cell::StaticCell;

use crate::queue::{ConfigQueues, HidQueues};

// pid.codes test IDs. A product ID of our own is needed before any release.
pub const VID: u16 = 0x1209;
pub const PID: u16 = 0x0001;
pub const MANUFACTURER: &str = "JMK";
pub const PRODUCT: &str = "JMK Keyboard";

/// Boot-compatible keyboard: modifiers, reserved byte, six keys, LED output.
const KEYBOARD_DESCRIPTOR: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, // Generic Desktop, Keyboard, Application
    0x05, 0x07, 0x19, 0xE0, 0x29, 0xE7, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, // modifiers
    0x95, 0x01, 0x75, 0x08, 0x81, 0x01, // reserved
    0x95, 0x05, 0x75, 0x01, 0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, // LEDs
    0x95, 0x01, 0x75, 0x03, 0x91, 0x01, // LED padding
    0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x05, 0x07, 0x19, 0x00, 0x2A, 0xFF, 0x00, 0x81, 0x00, // keys
    0xC0,
];

/// Media keys: one 16-bit consumer usage, 0 when nothing is pressed.
const CONSUMER_DESCRIPTOR: &[u8] = &[
    0x05, 0x0C, 0x09, 0x01, 0xA1, 0x01, // Consumer, Consumer Control, Application
    0x15, 0x00, 0x26, 0xFF, 0x03, 0x19, 0x00, 0x2A, 0xFF, 0x03, 0x75, 0x10, 0x95, 0x01, 0x81, 0x00,
    0xC0,
];

/// Vendor-defined 64-byte reports in both directions for the configurator.
const CONFIG_DESCRIPTOR: &[u8] = &[
    0x06, 0x4A, 0xFF, 0x09, 0x01, 0xA1, 0x01, // Vendor page 0xFF4A, usage 1, Application
    0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x40, // 64 bytes
    0x09, 0x02, 0x81, 0x02, // input
    0x09, 0x03, 0x91, 0x02, // output
    0xC0,
];

/// Where a collection's own items start: after usage page, usage and the
/// collection opener.
const COLLECTION_HEADER: usize = 6;
pub const REPORT_MAP_SIZE: usize = KEYBOARD_DESCRIPTOR.len() + CONSUMER_DESCRIPTOR.len() + 4;

/// The keyboard and consumer descriptors as one report map with report IDs,
/// for transports that carry all reports in one HID device (Bluetooth).
pub const fn report_map(keyboard_id: u8, consumer_id: u8) -> [u8; REPORT_MAP_SIZE] {
    let mut map = [0; REPORT_MAP_SIZE];
    let mut at = 0;
    let mut part = 0;
    while part < 2 {
        let (descriptor, id) = if part == 0 { (KEYBOARD_DESCRIPTOR, keyboard_id) } else { (CONSUMER_DESCRIPTOR, consumer_id) };
        let mut i = 0;
        while i < descriptor.len() {
            if i == COLLECTION_HEADER {
                map[at] = 0x85; // Report ID
                map[at + 1] = id;
                at += 2;
            }
            map[at] = descriptor[i];
            at += 1;
            i += 1;
        }
        part += 1;
    }
    map
}

static CONFIGURED: AtomicBool = AtomicBool::new(false);

/// True while a host has the device configured, which is when key reports
/// sent over USB arrive somewhere. Ports with a second transport route by it.
pub fn configured() -> bool {
    CONFIGURED.load(Ordering::Relaxed)
}

static SUSPENDED: AtomicBool = AtomicBool::new(false);

/// True while a computer is actually listening: configured and not
/// suspended. A pulled cable shows up as a suspended bus on chips that do
/// not watch the supply pin, and a sleeping computer suspends it too. Of two
/// connected halves of a split, the one this is true for does the typing.
pub fn listening() -> bool {
    configured() && !SUSPENDED.load(Ordering::Relaxed)
}

struct BusState;

impl Handler for BusState {
    fn enabled(&mut self, enabled: bool) {
        if !enabled {
            // The cable was pulled.
            CONFIGURED.store(false, Ordering::Relaxed);
        }
    }

    fn reset(&mut self) {
        CONFIGURED.store(false, Ordering::Relaxed);
        SUSPENDED.store(false, Ordering::Relaxed);
    }

    fn suspended(&mut self, suspended: bool) {
        SUSPENDED.store(suspended, Ordering::Relaxed);
    }

    fn configured(&mut self, configured: bool) {
        CONFIGURED.store(configured, Ordering::Relaxed);
    }
}

pub struct Usb<D: Driver<'static>> {
    device: UsbDevice<'static, D>,
    keyboard: Keyboard<D>,
    consumer: Consumer<D>,
    config: ConfigChannel<D>,
}

struct Keyboard<D: Driver<'static>>(HidReaderWriter<'static, D, 1, 8>);
struct Consumer<D: Driver<'static>>(HidWriter<'static, D, 2>);
struct ConfigChannel<D: Driver<'static>>(HidReaderWriter<'static, D, REPORT_SIZE, REPORT_SIZE>);

impl<D: Driver<'static>> Usb<D> {
    /// Must be called at most once.
    pub fn new(driver: D) -> Self {
        static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS_DESC: StaticCell<[u8; 64]> = StaticCell::new();
        static CONTROL_BUF: StaticCell<[u8; 128]> = StaticCell::new();
        static KEYBOARD_STATE: StaticCell<State> = StaticCell::new();
        static CONSUMER_STATE: StaticCell<State> = StaticCell::new();
        static CONFIG_STATE: StaticCell<State> = StaticCell::new();
        static BUS_STATE: StaticCell<BusState> = StaticCell::new();

        let mut config = Config::new(VID, PID);
        config.manufacturer = Some(MANUFACTURER);
        config.product = Some(PRODUCT);
        config.max_power = 100;

        let mut builder = Builder::new(
            driver,
            config,
            CONFIG_DESC.init([0; 256]),
            BOS_DESC.init([0; 64]),
            &mut [],
            CONTROL_BUF.init([0; 128]),
        );
        builder.handler(BUS_STATE.init(BusState));

        let keyboard = HidReaderWriter::new(
            &mut builder,
            KEYBOARD_STATE.init(State::new()),
            HidConfig {
                report_descriptor: KEYBOARD_DESCRIPTOR,
                request_handler: None,
                poll_ms: 1,
                max_packet_size: 8,
                hid_subclass: HidSubclass::Boot,
                hid_boot_protocol: HidBootProtocol::Keyboard,
            },
        );
        let consumer = HidWriter::new(
            &mut builder,
            CONSUMER_STATE.init(State::new()),
            HidConfig {
                report_descriptor: CONSUMER_DESCRIPTOR,
                request_handler: None,
                poll_ms: 4,
                max_packet_size: 8,
                hid_subclass: HidSubclass::No,
                hid_boot_protocol: HidBootProtocol::None,
            },
        );
        let config = HidReaderWriter::new(
            &mut builder,
            CONFIG_STATE.init(State::new()),
            HidConfig {
                report_descriptor: CONFIG_DESCRIPTOR,
                request_handler: None,
                poll_ms: 1,
                max_packet_size: REPORT_SIZE as u16,
                hid_subclass: HidSubclass::No,
                hid_boot_protocol: HidBootProtocol::None,
            },
        );

        Usb {
            device: builder.build(),
            keyboard: Keyboard(keyboard),
            consumer: Consumer(consumer),
            config: ConfigChannel(config),
        }
    }

    /// Run the bus and all three interfaces, forever.
    pub async fn run(mut self, hid: &HidQueues, config: &ConfigQueues) {
        join(
            join(self.device.run(), self.config.run(config)),
            join(self.keyboard.run(hid), self.consumer.run(hid)),
        )
        .await;
    }
}

impl<D: Driver<'static>> Keyboard<D> {
    /// Send queued keyboard reports to the host.
    async fn run(self, queues: &HidQueues) {
        let (mut reader, mut writer) = self.0.split();
        let send = async {
            loop {
                let report = queues.keyboard.receive().await;
                // Fails only while the bus is down; the next report carries
                // the complete state again.
                let _ = writer.write(&report).await;
            }
        };
        let leds = async {
            let mut buf = [0u8; 1];
            loop {
                // Lock-key LED state. Nothing shows it yet.
                let _ = reader.read(&mut buf).await;
            }
        };
        join(send, leds).await;
    }
}

impl<D: Driver<'static>> Consumer<D> {
    /// Send queued media key changes to the host.
    async fn run(mut self, queues: &HidQueues) {
        loop {
            let usage = queues.consumer.receive().await;
            let _ = self.0.write(&usage.to_le_bytes()).await;
        }
    }
}

impl<D: Driver<'static>> ConfigChannel<D> {
    /// Move configurator frames between the host and the two queues.
    async fn run(self, queues: &ConfigQueues) {
        let (mut reader, mut writer) = self.0.split();
        let send = async {
            loop {
                let report = queues.to_host.receive().await;
                let _ = writer.write(&report).await;
            }
        };
        let receive = async {
            let mut buf = [0u8; REPORT_SIZE];
            loop {
                if let Ok(len) = reader.read(&mut buf).await {
                    let mut report = [0u8; REPORT_SIZE];
                    report[..len].copy_from_slice(&buf[..len]);
                    queues.from_host.send(report).await;
                }
            }
        };
        join(send, receive).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_map_is_both_descriptors_with_report_ids() {
        let map = report_map(1, 2);
        let keyboard_end = KEYBOARD_DESCRIPTOR.len() + 2;
        let (keyboard, consumer) = map.split_at(keyboard_end);
        for (part, descriptor, id) in [(keyboard, KEYBOARD_DESCRIPTOR, 1), (consumer, CONSUMER_DESCRIPTOR, 2)] {
            assert_eq!(part[..COLLECTION_HEADER], descriptor[..COLLECTION_HEADER]);
            assert_eq!(part[COLLECTION_HEADER..COLLECTION_HEADER + 2], [0x85, id]);
            assert_eq!(part[COLLECTION_HEADER + 2..], descriptor[COLLECTION_HEADER..]);
            // The ID must come directly after the collection opens.
            assert_eq!(descriptor[COLLECTION_HEADER - 2..COLLECTION_HEADER], [0xA1, 0x01]);
        }
    }
}
