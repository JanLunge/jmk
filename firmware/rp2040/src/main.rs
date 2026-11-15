//! JMK on the RP2040: USB keyboard plus the configurator channel.
//!
//! All keyboard behaviour lives in `jmk-device`. This file only connects it
//! to the chip: GPIO by number, flash, USB and the clock.
#![no_std]
#![no_main]

mod link;
mod pins;
mod usb;

use embassy_executor::Spawner;
use embassy_futures::join::{join, join5};
use embassy_futures::select::{select, Either};
use embassy_futures::yield_now;
use embassy_rp::flash::{Blocking, Flash};
use embassy_rp::peripherals::FLASH;
use embassy_rp::watchdog::Watchdog;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Ticker};
use jmk_core::{KeyboardReport, Output};
use jmk_device::{Board, Config, Device, Storage, STORAGE_SIZE};
use jmk_proto::frame::{self, Assembler, REPORT_SIZE};
use jmk_proto::{DeviceMessage, Error, Mcu, Request, Response};
use panic_halt as _;
use static_cell::ConstStaticCell;

const FLASH_SIZE: usize = 2 * 1024 * 1024;
/// Must match the space left free at the end of FLASH in memory.x.
const STORAGE_OFFSET: u32 = FLASH_SIZE as u32 - STORAGE_SIZE;

type KeyboardQueue = Channel<NoopRawMutex, [u8; 8], 32>;
type ConsumerQueue = Channel<NoopRawMutex, u16, 8>;
type FrameQueue = Channel<NoopRawMutex, [u8; REPORT_SIZE], 16>;

static CONFIG: ConstStaticCell<Config> = ConstStaticCell::new(Config::new());
static STORAGE_BUF: ConstStaticCell<[u8; 16 * 1024]> = ConstStaticCell::new([0; 16 * 1024]);

struct HidOutput<'a> {
    keyboard: &'a KeyboardQueue,
    consumer: &'a ConsumerQueue,
}

impl Output for HidOutput<'_> {
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
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let keyboard_queue = KeyboardQueue::new();
    let consumer_queue = ConsumerQueue::new();
    let link_queues = link::Queues::new();
    let to_host = FrameQueue::new();
    let from_host = FrameQueue::new();

    let mut flash: Flash<'_, FLASH, Blocking, FLASH_SIZE> = Flash::new_blocking(p.FLASH);
    let mut uid = [0u8; 8];
    // Left at zero if the flash chip does not report an ID.
    let _ = flash.blocking_unique_id(&mut uid);
    let storage = Storage::new(flash, STORAGE_OFFSET, STORAGE_BUF.take());
    let board = Board { mcu: Mcu::Rp2040, firmware: firmware_version(), uid };
    let mut device = Device::new(board, pins::RpPins::new(), storage, CONFIG.take());
    let mut out = HidOutput { keyboard: &keyboard_queue, consumer: &consumer_queue };
    let serial = device.hardware().split.link().and_then(|config| link::SerialLink::open(&config));

    let send = |message: &DeviceMessage| {
        // If the configurator is not reading, messages are dropped and it
        // has to ask again.
        let _ = frame::encode(message, |report| {
            let _ = to_host.try_send(*report);
        });
    };

    // Resets the chip if the main loop stops, so a hang never needs a hand
    // on the board. Saving to flash blocks for a few hundred milliseconds.
    const WATCHDOG_TIMEOUT: Duration = Duration::from_secs(3);
    let mut watchdog = Watchdog::new(p.WATCHDOG);
    watchdog.start(WATCHDOG_TIMEOUT);

    let run = async {
        let mut ticker = Ticker::every(Duration::from_millis(1));
        let mut assembler = Assembler::new();
        let mut reboot: Option<(Instant, bool)> = None;
        loop {
            watchdog.feed(WATCHDOG_TIMEOUT);
            // If a pass overruns its millisecond the ticker is ready again at
            // once; yielding keeps USB and the other futures running.
            yield_now().await;
            match select(ticker.next(), from_host.receive()).await {
                Either::First(()) => {
                    let now = Instant::now();
                    device.poll(now.as_millis(), &mut out, &mut &link_queues, |event| {
                        send(&DeviceMessage::Event(event))
                    });
                    if let Some((at, bootloader)) = reboot {
                        if now >= at {
                            reboot_now(bootloader);
                        }
                    }
                }
                Either::Second(report) => {
                    let Some(request) = assembler.push::<Request>(&report) else { continue };
                    let response = match request {
                        Ok(request) => device.handle(request, &mut out),
                        Err(_) => Response::Error(Error::Malformed),
                    };
                    send(&DeviceMessage::Response(response));
                    if let Some(bootloader) = device.take_reboot() {
                        // Give the reply time to reach the host first.
                        reboot = Some((Instant::now() + Duration::from_millis(100), bootloader));
                    }
                }
            }
        }
    };

    let mut usb = usb::Usb::new(p.USB);
    let cable = async {
        match serial {
            Some(serial) => serial.run(&link_queues).await,
            None => core::future::pending().await,
        }
    };
    join5(
        run,
        usb.device.run(),
        join(usb.keyboard.run(&keyboard_queue), usb.consumer.run(&consumer_queue)),
        usb.config.run(&to_host, &from_host),
        cable,
    )
    .await;
}

fn firmware_version() -> (u8, u8, u8) {
    let part = |s: &str| s.parse().unwrap_or(0);
    (
        part(env!("CARGO_PKG_VERSION_MAJOR")),
        part(env!("CARGO_PKG_VERSION_MINOR")),
        part(env!("CARGO_PKG_VERSION_PATCH")),
    )
}

fn reboot_now(bootloader: bool) -> ! {
    if bootloader {
        embassy_rp::rom_data::reset_to_usb_boot(0, 0);
    }
    cortex_m::peripheral::SCB::sys_reset()
}
