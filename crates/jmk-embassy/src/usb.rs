//! The USB side every port presents: a boot keyboard, consumer control, the
//! vendor channel for the configurator and a mouse.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

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

/// Five buttons, X and Y as 16 bits, wheel, horizontal wheel (AC Pan). Not
/// the boot mouse: that has 8-bit X and Y only. Some computers keep a
/// device's descriptor once read, so change it only with good reason.
///
/// One item per line, named as in the USB HID 1.11 specification (§6.2.2,
/// https://www.usb.org/sites/default/files/hid1_11.pdf); the usages are from
/// the HID Usage Tables (https://www.usb.org/sites/default/files/hut1_7.pdf).
/// Each item is a prefix byte, then 0 to 2 bytes of data, little-endian.
/// The report it describes is [`jmk_core::MouseReport::bytes`].
const MOUSE_DESCRIPTOR: &[u8] = &[
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x02, // Usage (Mouse)
    0xA1, 0x01, // Collection (Application)
    0x09, 0x01, //   Usage (Pointer)
    0xA1, 0x00, //   Collection (Physical)
    //
    // Byte 0: buttons 1 to 5, one bit each, then 3 bits of padding.
    0x05, 0x09, //     Usage Page (Button)
    0x19, 0x01, //     Usage Minimum (Button 1)
    0x29, 0x05, //     Usage Maximum (Button 5)
    0x15, 0x00, //     Logical Minimum (0)
    0x25, 0x01, //     Logical Maximum (1)
    0x95, 0x05, //     Report Count (5)
    0x75, 0x01, //     Report Size (1 bit)
    0x81, 0x02, //     Input (Data, Variable, Absolute)
    0x95, 0x01, //     Report Count (1)
    0x75, 0x03, //     Report Size (3 bits)
    0x81, 0x01, //     Input (Constant): padding
    //
    // Bytes 1 to 4: X, then Y, 16 bits each, movement since the last report.
    0x05, 0x01, //     Usage Page (Generic Desktop)
    0x09, 0x30, //     Usage (X)
    0x09, 0x31, //     Usage (Y)
    0x16, 0x01, 0x80, //     Logical Minimum (-32767)
    0x26, 0xFF, 0x7F, //     Logical Maximum (32767)
    0x75, 0x10, //     Report Size (16 bits)
    0x95, 0x02, //     Report Count (2)
    0x81, 0x06, //     Input (Data, Variable, Relative)
    //
    // Byte 5: the wheel, steps since the last report, up is positive.
    0x09, 0x38, //     Usage (Wheel)
    0x15, 0x81, //     Logical Minimum (-127)
    0x25, 0x7F, //     Logical Maximum (127)
    0x75, 0x08, //     Report Size (8 bits)
    0x95, 0x01, //     Report Count (1)
    0x81, 0x06, //     Input (Data, Variable, Relative)
    //
    // Byte 6: horizontal scrolling, right is positive.
    0x05, 0x0C, //     Usage Page (Consumer)
    0x0A, 0x38, 0x02, //     Usage (AC Pan, 0x0238)
    0x15, 0x81, //     Logical Minimum (-127)
    0x25, 0x7F, //     Logical Maximum (127)
    0x75, 0x08, //     Report Size (8 bits)
    0x95, 0x01, //     Report Count (1)
    0x81, 0x06, //     Input (Data, Variable, Relative)
    //
    0xC0, //   End Collection (Physical)
    0xC0, // End Collection (Application)
];
/// Bytes in a mouse report, see [`jmk_core::MouseReport::bytes`].
const MOUSE_REPORT_SIZE: usize = 7;

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

/// The descriptors a report map joins, in the order of `report_map`'s IDs.
const REPORT_MAP_PARTS: [&[u8]; 3] = [KEYBOARD_DESCRIPTOR, CONSUMER_DESCRIPTOR, MOUSE_DESCRIPTOR];
/// Prefix of a Report ID item with one byte of data (HID 1.11, §6.2.2.7).
const REPORT_ID_PREFIX: u8 = 0x85;
/// The Report ID item `report_map` puts into each descriptor: the prefix,
/// then the ID.
const REPORT_ID_ITEM: usize = 2;
/// Every descriptor in `REPORT_MAP_PARTS`, each with its Report ID item.
pub const REPORT_MAP_SIZE: usize = {
    let mut size = 0;
    let mut part = 0;
    while part < REPORT_MAP_PARTS.len() {
        size += REPORT_MAP_PARTS[part].len() + REPORT_ID_ITEM;
        part += 1;
    }
    size
};

/// The keyboard, consumer and mouse descriptors as one report map with
/// report IDs, for transports that carry all reports in one HID device
/// (Bluetooth).
pub const fn report_map(keyboard_id: u8, consumer_id: u8, mouse_id: u8) -> [u8; REPORT_MAP_SIZE] {
    let ids = [keyboard_id, consumer_id, mouse_id];
    let mut map = [0; REPORT_MAP_SIZE];
    let mut at = 0;
    let mut part = 0;
    while part < REPORT_MAP_PARTS.len() {
        let (descriptor, id) = (REPORT_MAP_PARTS[part], ids[part]);
        let mut i = 0;
        while i < descriptor.len() {
            if i == COLLECTION_HEADER {
                map[at] = REPORT_ID_PREFIX;
                map[at + 1] = id;
                at += REPORT_ID_ITEM;
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

static LOCKS: AtomicU8 = AtomicU8::new(0);

/// The lock keys as the computer last reported them: bit 0 num lock, 1 caps
/// lock, 2 scroll lock.
pub fn locks() -> u8 {
    LOCKS.load(Ordering::Relaxed)
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
    mouse: Mouse<D>,
}

struct Keyboard<D: Driver<'static>>(HidReaderWriter<'static, D, 1, 8>);
struct Consumer<D: Driver<'static>>(HidWriter<'static, D, 2>);
struct ConfigChannel<D: Driver<'static>>(HidReaderWriter<'static, D, REPORT_SIZE, REPORT_SIZE>);
struct Mouse<D: Driver<'static>>(HidWriter<'static, D, MOUSE_REPORT_SIZE>);

impl<D: Driver<'static>> Usb<D> {
    /// Must be called at most once.
    pub fn new(driver: D) -> Self {
        static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS_DESC: StaticCell<[u8; 64]> = StaticCell::new();
        static CONTROL_BUF: StaticCell<[u8; 128]> = StaticCell::new();
        static KEYBOARD_STATE: StaticCell<State> = StaticCell::new();
        static CONSUMER_STATE: StaticCell<State> = StaticCell::new();
        static CONFIG_STATE: StaticCell<State> = StaticCell::new();
        static MOUSE_STATE: StaticCell<State> = StaticCell::new();
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

        // The order is part of what a computer is told: on Linux the udev
        // rule (app/linux/70-jmk-keyboard.rules) lets programs of the user
        // at the third interface, the settings channel, and at no other.
        // A new interface goes after these three; the mouse is the first
        // that did.
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

        let mouse = HidWriter::new(
            &mut builder,
            MOUSE_STATE.init(State::new()),
            HidConfig {
                report_descriptor: MOUSE_DESCRIPTOR,
                request_handler: None,
                poll_ms: 1,
                max_packet_size: 8,
                hid_subclass: HidSubclass::No,
                hid_boot_protocol: HidBootProtocol::None,
            },
        );

        Usb {
            device: builder.build(),
            keyboard: Keyboard(keyboard),
            consumer: Consumer(consumer),
            config: ConfigChannel(config),
            mouse: Mouse(mouse),
        }
    }

    /// Run the bus and all four interfaces, forever.
    pub async fn run(mut self, hid: &HidQueues, config: &ConfigQueues) {
        join(
            join(self.device.run(), self.config.run(config)),
            join(join(self.keyboard.run(hid), self.consumer.run(hid)), self.mouse.run(hid)),
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
                // Lock-key LED state, for the indicator lights.
                if reader.read(&mut buf).await.is_ok() {
                    LOCKS.store(buf[0], Ordering::Relaxed);
                }
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

impl<D: Driver<'static>> Mouse<D> {
    /// Send queued mouse reports to the host.
    async fn run(mut self, queues: &HidQueues) {
        loop {
            let report = queues.mouse.receive().await;
            let _ = self.0.write(&report).await;
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
    fn report_map_is_all_descriptors_with_report_ids() {
        let map = report_map(1, 2, 3);
        let (keyboard, rest) = map.split_at(KEYBOARD_DESCRIPTOR.len() + 2);
        let (consumer, mouse) = rest.split_at(CONSUMER_DESCRIPTOR.len() + 2);
        for (part, descriptor, id) in [(keyboard, KEYBOARD_DESCRIPTOR, 1), (consumer, CONSUMER_DESCRIPTOR, 2), (mouse, MOUSE_DESCRIPTOR, 3)] {
            assert_eq!(part[..COLLECTION_HEADER], descriptor[..COLLECTION_HEADER]);
            assert_eq!(part[COLLECTION_HEADER..COLLECTION_HEADER + 2], [0x85, id]);
            assert_eq!(part[COLLECTION_HEADER + 2..], descriptor[COLLECTION_HEADER..]);
            // The ID must come directly after the collection opens.
            assert_eq!(descriptor[COLLECTION_HEADER - 2..COLLECTION_HEADER], [0xA1, 0x01]);
        }
    }
}
