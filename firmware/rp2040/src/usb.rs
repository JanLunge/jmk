use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_rp::Peri;
use embassy_usb::class::hid::{Config as HidConfig, HidBootProtocol, HidReaderWriter, HidSubclass, State};
use embassy_usb::{Builder, Config, UsbDevice};
use embassy_futures::join::join;
use jmk_proto::frame::REPORT_SIZE;
use static_cell::StaticCell;

use crate::{FrameQueue, KeyboardQueue};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

// pid.codes test IDs. A product ID of our own is needed before any release.
const VID: u16 = 0x1209;
const PID: u16 = 0x0001;

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

/// Vendor-defined 64-byte reports in both directions for the configurator.
const CONFIG_DESCRIPTOR: &[u8] = &[
    0x06, 0x4A, 0xFF, 0x09, 0x01, 0xA1, 0x01, // Vendor page 0xFF4A, usage 1, Application
    0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x40, // 64 bytes
    0x09, 0x02, 0x81, 0x02, // input
    0x09, 0x03, 0x91, 0x02, // output
    0xC0,
];

type RpDriver = Driver<'static, USB>;

pub struct Usb {
    pub device: UsbDevice<'static, RpDriver>,
    pub keyboard: Keyboard,
    pub config: ConfigChannel,
}

pub struct Keyboard(HidReaderWriter<'static, RpDriver, 1, 8>);
pub struct ConfigChannel(HidReaderWriter<'static, RpDriver, REPORT_SIZE, REPORT_SIZE>);

impl Usb {
    pub fn new(usb: Peri<'static, USB>) -> Self {
        static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS_DESC: StaticCell<[u8; 64]> = StaticCell::new();
        static CONTROL_BUF: StaticCell<[u8; 128]> = StaticCell::new();
        static KEYBOARD_STATE: StaticCell<State> = StaticCell::new();
        static CONFIG_STATE: StaticCell<State> = StaticCell::new();

        let mut config = Config::new(VID, PID);
        config.manufacturer = Some("JMK");
        config.product = Some("JMK Keyboard");
        config.max_power = 100;

        let mut builder = Builder::new(
            Driver::new(usb, Irqs),
            config,
            CONFIG_DESC.init([0; 256]),
            BOS_DESC.init([0; 64]),
            &mut [],
            CONTROL_BUF.init([0; 128]),
        );

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

        Usb { device: builder.build(), keyboard: Keyboard(keyboard), config: ConfigChannel(config) }
    }
}

impl Keyboard {
    /// Send queued keyboard reports to the host.
    pub async fn run(self, queue: &KeyboardQueue) {
        let (mut reader, mut writer) = self.0.split();
        let send = async {
            loop {
                let report = queue.receive().await;
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

impl ConfigChannel {
    /// Move configurator frames between the host and the two queues.
    pub async fn run(self, to_host: &FrameQueue, from_host: &FrameQueue) {
        let (mut reader, mut writer) = self.0.split();
        let send = async {
            loop {
                let report = to_host.receive().await;
                let _ = writer.write(&report).await;
            }
        };
        let receive = async {
            let mut buf = [0u8; REPORT_SIZE];
            loop {
                if let Ok(len) = reader.read(&mut buf).await {
                    let mut report = [0u8; REPORT_SIZE];
                    report[..len].copy_from_slice(&buf[..len]);
                    from_host.send(report).await;
                }
            }
        };
        join(send, receive).await;
    }
}
