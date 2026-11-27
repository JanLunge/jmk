//! JMK on the RP2040 and the RP2350: USB keyboard plus the configurator
//! channel.
//!
//! All keyboard behaviour lives in `jmk-device`, and what Embassy ports have
//! in common in `jmk-embassy`. This file only connects them to the chip:
//! GPIO by number, flash, USB and the clock.
//!
//! `firmware/rp2350` builds this same code with its feature `rp235x` (and
//! `rp235xb` for the package with 48 GPIOs); the few places where the chips
//! differ are marked with it.
#![no_std]
#![no_main]

mod lights;
mod link;
mod pins;

use embassy_executor::Spawner;
use embassy_futures::join::join4;
use embassy_rp::bind_interrupts;
use embassy_rp::flash::{Blocking, Flash};
use embassy_rp::peripherals::{FLASH, USB};
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_rp::watchdog::Watchdog;
use embassy_time::Duration;
use jmk_device::{Board, Config, Device, Storage, STORAGE_SIZE};
use jmk_embassy::queue::{ConfigQueues, HidQueues};
use jmk_embassy::usb::Usb;
use jmk_proto::Mcu;
use panic_halt as _;
use static_cell::ConstStaticCell;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

/// The part of the flash this firmware uses, on both chips; a larger flash
/// chip is left alone beyond it.
const FLASH_SIZE: usize = 2 * 1024 * 1024;
/// Must match the space left free at the end of FLASH in memory.x.
const STORAGE_OFFSET: u32 = FLASH_SIZE as u32 - STORAGE_SIZE;

static CONFIG: ConstStaticCell<Config> = ConstStaticCell::new(Config::new());
static STORAGE_BUF: ConstStaticCell<[u8; 16 * 1024]> = ConstStaticCell::new([0; 16 * 1024]);

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let hid_queues = HidQueues::new();
    let config_queues = ConfigQueues::new();
    let link_queues = jmk_embassy::link::Queues::new();

    #[cfg(not(feature = "rp235x"))]
    let (mcu, mut flash) = (Mcu::Rp2040, Flash::<'_, FLASH, Blocking, FLASH_SIZE>::new_blocking(p.FLASH));
    #[cfg(feature = "rp235x")]
    let (mcu, flash) = (Mcu::Rp2350, Flash::<'_, FLASH, Blocking, FLASH_SIZE>::new_blocking(p.FLASH));
    let mut uid = [0u8; 8];
    // Left at zero if the flash chip does not report an ID.
    #[cfg(not(feature = "rp235x"))]
    let _ = flash.blocking_unique_id(&mut uid);
    // The RP2350 carries a number of its own, written at the factory.
    #[cfg(feature = "rp235x")]
    if let Ok(id) = embassy_rp::otp::get_chipid() {
        uid = id.to_be_bytes();
    }
    let storage = Storage::new(flash, STORAGE_OFFSET, STORAGE_BUF.take());
    let board = Board { mcu, firmware: jmk_proto::FIRMWARE_VERSION, uid };
    let mut device = Device::new(board, pins::RpPins::new(), storage, CONFIG.take());
    let wiring = jmk_embassy::wiring::Wiring::new();

    // Saving to flash blocks for a few hundred milliseconds.
    const WATCHDOG_TIMEOUT: Duration = Duration::from_secs(3);
    let mut watchdog = Watchdog::new(p.WATCHDOG);
    watchdog.start(WATCHDOG_TIMEOUT);

    let run = jmk_embassy::run::run(
        &mut device,
        &hid_queues,
        &link_queues,
        &config_queues,
        &wiring,
        || watchdog.feed(WATCHDOG_TIMEOUT),
        jmk_embassy::usb::listening,
        reboot,
    );
    let usb = Usb::new(Driver::new(p.USB, Irqs));
    let cable = link::run(&wiring, &link_queues, p.PIO1);
    let lights = lights::run(p.PIO0, p.DMA_CH0, &wiring);
    join4(run, usb.run(&hid_queues, &config_queues), cable, lights).await;
}

fn reboot(bootloader: bool) -> ! {
    if bootloader {
        embassy_rp::rom_data::reset_to_usb_boot(0, 0);
    }
    cortex_m::peripheral::SCB::sys_reset()
}
