//! JMK on the nRF52840: USB keyboard, Bluetooth keyboard and the
//! configurator channel.
//!
//! All keyboard behaviour lives in `jmk-device`, and what Embassy ports have
//! in common in `jmk-embassy`. This crate connects them to the chip: GPIO by
//! number, flash, USB, the radio and the clocks.
//!
//! Nothing here has run on hardware yet; README.md lists what to test.
#![no_std]
#![no_main]

mod battery;
mod ble;
mod bonds;
mod flash;
mod link;
mod pins;

use embassy_executor::Spawner;
use embassy_futures::join::join5;
use embassy_nrf::config::Config as ChipConfig;
use embassy_nrf::interrupt::{self, InterruptExt, Priority};
use embassy_nrf::peripherals::{RNG, USBD, WDT};
use embassy_nrf::usb::vbus_detect::HardwareVbusDetect;
use embassy_nrf::usb::Driver;
use embassy_nrf::wdt::{self, HaltConfig, Watchdog, WatchdogHandle};
use embassy_nrf::{bind_interrupts, pac, rng, usb};
use jmk_core::{KeyboardReport, MouseReport, Output};
use jmk_device::{Board, Config, Device, Storage};
use jmk_embassy::queue::{ConfigQueues, HidQueues};
use jmk_embassy::usb::Usb;
use jmk_proto::Mcu;
use nrf_mpsl::{MultiprotocolServiceLayer, SessionMem};
use nrf_sdc::SoftdeviceController;
use panic_halt as _;
use static_cell::{ConstStaticCell, StaticCell};

bind_interrupts!(struct Irqs {
    USBD => usb::InterruptHandler<USBD>;
    RNG => rng::InterruptHandler<RNG>;
    // The clock belongs to the radio scheduler; USB power detection shares
    // its interrupt.
    CLOCK_POWER => nrf_mpsl::ClockInterruptHandler, usb::vbus_detect::InterruptHandler;
    EGU0_SWI0 => nrf_mpsl::LowPrioInterruptHandler;
    RADIO => nrf_mpsl::HighPrioInterruptHandler;
    TIMER0 => nrf_mpsl::HighPrioInterruptHandler;
    RTC0 => nrf_mpsl::HighPrioInterruptHandler;
});

/// The radio scheduler's interrupts run at priority 0 and must not be held
/// up, so everything of ours runs below them.
const APP_PRIORITY: Priority = Priority::P2;

/// Largest packet exchanged with the host and how many the controller
/// queues per direction; the values of the trouble-host nRF52 examples.
const PACKET_SIZE: u16 = 72;
const PACKET_COUNT: u8 = 3;
/// Memory for the Bluetooth controller, which refuses to start with too
/// little. The size macros in its header (sdc.h) give about 2.1 KiB for one
/// advertising set and one peripheral link with these buffers; the rest is
/// margin, since the exact need cannot be checked without running it.
const CONTROLLER_MEMORY: usize = 6144;

static CONFIG: ConstStaticCell<Config> = ConstStaticCell::new(Config::new());
static STORAGE_BUF: ConstStaticCell<[u8; 16 * 1024]> = ConstStaticCell::new([0; 16 * 1024]);

/// Sends key reports over USB while a computer has the keyboard configured
/// on the bus, and over Bluetooth otherwise.
struct RoutedOutput<'a> {
    usb: &'a HidQueues,
    ble: &'a HidQueues,
    over_usb: bool,
}

impl<'a> RoutedOutput<'a> {
    fn route(&mut self) -> &'a HidQueues {
        let queues = |over_usb| if over_usb { self.usb } else { self.ble };
        let over_usb = jmk_embassy::usb::configured();
        if over_usb != self.over_usb {
            // Keys held on the transport being left would stay down there.
            let mut left = queues(self.over_usb);
            left.keyboard(&KeyboardReport::default());
            left.consumer(0);
            left.mouse(&MouseReport::default());
            self.over_usb = over_usb;
        }
        queues(over_usb)
    }
}

impl Output for RoutedOutput<'_> {
    fn keyboard(&mut self, report: &KeyboardReport) {
        self.route().keyboard(report);
    }

    fn consumer(&mut self, usage: u16) {
        self.route().consumer(usage);
    }

    fn mouse(&mut self, report: &MouseReport) {
        self.route().mouse(report);
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut chip = ChipConfig::default();
    chip.time_interrupt_priority = APP_PRIORITY;
    chip.lfclk_source = lfclk::CHIP_SOURCE;
    let p = embassy_nrf::init(chip);
    interrupt::USBD.set_priority(APP_PRIORITY);
    interrupt::RNG.set_priority(APP_PRIORITY);
    interrupt::UARTE0.set_priority(APP_PRIORITY);

    // The radio scheduler comes first: writing flash goes through it.
    static MPSL: StaticCell<MultiprotocolServiceLayer<'static>> = StaticCell::new();
    static SESSION: StaticCell<SessionMem<1>> = StaticCell::new();
    let mpsl_peripherals = nrf_mpsl::Peripherals::new(p.RTC0, p.TIMER0, p.TEMP, p.PPI_CH19, p.PPI_CH30, p.PPI_CH31);
    // Fails only for a clock configuration the library rejects, and then
    // nothing below can work: halt and let the user reach the bootloader
    // with the reset button.
    let mpsl: &'static MultiprotocolServiceLayer<'static> = MPSL.init(
        MultiprotocolServiceLayer::with_timeslots(mpsl_peripherals, Irqs, lfclk::config(), SESSION.init(SessionMem::new()))
            .unwrap(),
    );
    static FLASH: StaticCell<flash::Flash> = StaticCell::new();
    let flash: &'static flash::Flash = FLASH.init(flash::Flash::new(mpsl, p.NVMC));

    let usb_reports = HidQueues::new();
    let ble_reports = HidQueues::new();
    let config_queues = ConfigQueues::new();
    let link_queues = jmk_embassy::link::Queues::new();

    let storage = Storage::new(flash, flash::CONFIG_OFFSET, STORAGE_BUF.take());
    let board = Board { mcu: Mcu::Nrf52840, firmware: jmk_proto::FIRMWARE_VERSION, uid: device_id() };
    let mut device = Device::new(board, pins::NrfPins::new(), storage, CONFIG.take());
    let wiring = jmk_embassy::wiring::Wiring::new();

    // Saving erases flash in 10 ms time slots between radio events, 36 of
    // them for a save and 72 for a factory reset. With a connection that
    // leaves little room this can take a few seconds.
    const WATCHDOG_SECONDS: u32 = 10;
    let mut watchdog = start_watchdog(p.WDT, WATCHDOG_SECONDS);

    let run = jmk_embassy::run::run(
        &mut device,
        RoutedOutput { usb: &usb_reports, ble: &ble_reports, over_usb: false },
        &link_queues,
        &config_queues,
        &wiring,
        || watchdog.iter_mut().for_each(WatchdogHandle::pet),
        || jmk_embassy::usb::listening() || ble::connected(),
        reboot,
    );
    let usb = async {
        // USB needs the crystal oscillator, which the radio scheduler
        // otherwise only runs around radio events. The request cannot fail:
        // it is the only one. Dropping the guard would release the crystal.
        let _crystal = mpsl.request_hfclk().await;
        let _ = nrf_mpsl::Hfclk::wait().await;
        let driver = Driver::new(p.USBD, Irqs, HardwareVbusDetect::new(Irqs));
        Usb::new(driver).run(&usb_reports, &config_queues).await;
    };
    let bluetooth = async {
        let controller_peripherals = nrf_sdc::Peripherals::new(
            p.PPI_CH17, p.PPI_CH18, p.PPI_CH20, p.PPI_CH21, p.PPI_CH22, p.PPI_CH23, p.PPI_CH24, p.PPI_CH25,
            p.PPI_CH26, p.PPI_CH27, p.PPI_CH28, p.PPI_CH29,
        );
        let mut rng = rng::Rng::new(p.RNG, Irqs);
        let mut memory = nrf_sdc::Mem::<CONTROLLER_MEMORY>::new();
        if let Ok(controller) = controller(controller_peripherals, &mut rng, mpsl, &mut memory) {
            ble::run(controller, bluetooth_address(), bonds::Bonds::load(flash), &ble_reports).await;
        }
        // Without Bluetooth the keyboard still works over USB.
        core::future::pending::<()>().await;
    };
    let cable = link::run(&wiring, &link_queues);
    join5(run, usb, bluetooth, cable, mpsl.run()).await;
}

/// The low-frequency clock: the time base of the radio and of embassy-time.
///
/// Build-time choice. The default is the internal RC oscillator, which works
/// on every board. Building with `--features lfxo` uses a 32.768 kHz crystal
/// on P0.00/P0.01 instead, which saves power but stops the firmware at start
/// on a board without that crystal.
mod lfclk {
    use embassy_nrf::config::LfclkSource;
    use nrf_mpsl::raw::*;

    #[cfg(not(feature = "lfxo"))]
    pub const CHIP_SOURCE: LfclkSource = LfclkSource::InternalRC;
    #[cfg(feature = "lfxo")]
    pub const CHIP_SOURCE: LfclkSource = LfclkSource::ExternalXtal;

    pub fn config() -> mpsl_clock_lfclk_cfg_t {
        // Calibration intervals apply to the RC oscillator only and must be
        // zero for a crystal (mpsl_clock.h).
        #[cfg(not(feature = "lfxo"))]
        let (source, rc_ctiv, rc_temp_ctiv) =
            (MPSL_CLOCK_LF_SRC_RC, MPSL_RECOMMENDED_RC_CTIV, MPSL_RECOMMENDED_RC_TEMP_CTIV);
        #[cfg(feature = "lfxo")]
        let (source, rc_ctiv, rc_temp_ctiv) = (MPSL_CLOCK_LF_SRC_XTAL, 0, 0);
        mpsl_clock_lfclk_cfg_t {
            source: source as u8,
            rc_ctiv: rc_ctiv as u8,
            rc_temp_ctiv: rc_temp_ctiv as u8,
            accuracy_ppm: MPSL_DEFAULT_CLOCK_ACCURACY_PPM as u16,
            skip_wait_lfclk_started: MPSL_DEFAULT_SKIP_WAIT_LFCLK_STARTED != 0,
        }
    }
}

/// The Bluetooth controller for one connection as a peripheral.
fn controller<'d>(
    peripherals: nrf_sdc::Peripherals<'d>,
    rng: &'d mut rng::Rng<'static, embassy_nrf::mode::Async>,
    mpsl: &'d MultiprotocolServiceLayer<'_>,
    memory: &'d mut nrf_sdc::Mem<CONTROLLER_MEMORY>,
) -> Result<SoftdeviceController<'d>, nrf_sdc::Error> {
    nrf_sdc::Builder::new()?
        .support_adv()
        .support_peripheral()
        // Resolving lists, so that a bonded host using private addresses is
        // recognised; the host stack syncs them whenever a bond is added.
        .support_le_privacy()
        .peripheral_count(1)?
        .buffer_cfg(PACKET_SIZE, PACKET_SIZE, PACKET_COUNT, PACKET_COUNT)?
        .build(peripherals, rng, mpsl, memory)
}

/// Start the watchdog and return what feeds it.
///
/// Once started it cannot be stopped or reconfigured and keeps running
/// through a soft reset, so it is still counting when the bootloader takes
/// over; the Adafruit bootloader feeds all eight channels for that reason
/// (`wait_for_events` in its bootloader.c). If other firmware left it
/// running with settings of its own, do the same instead of failing.
fn start_watchdog(wdt: embassy_nrf::Peri<'static, WDT>, seconds: u32) -> [WatchdogHandle; 8] {
    let mut config = wdt::Config::default();
    config.timeout_ticks = 32768 * seconds;
    config.action_during_debug_halt = HaltConfig::Pause;
    match Watchdog::try_new(wdt, config) {
        Ok((_, handles)) => handles,
        // SAFETY: nothing else feeds the watchdog.
        Err(_) => core::array::from_fn(|channel| unsafe { WatchdogHandle::steal::<WDT>(channel as u8) }),
    }
}

/// The identifier the chip was given in the factory.
fn device_id() -> [u8; 8] {
    let mut id = [0u8; 8];
    id[..4].copy_from_slice(&pac::FICR.deviceid(0).read().to_le_bytes());
    id[4..].copy_from_slice(&pac::FICR.deviceid(1).read().to_le_bytes());
    id
}

/// The chip's factory-programmed random address, as a static random
/// address: the two top bits set (Bluetooth Core, Vol 6, Part B, 1.3.2.1).
fn bluetooth_address() -> [u8; 6] {
    let low = pac::FICR.deviceaddr(0).read().to_le_bytes();
    let high = pac::FICR.deviceaddr(1).read().to_le_bytes();
    [low[0], low[1], low[2], low[3], high[0], high[1] | 0xC0]
}

/// GPREGRET value that makes the Adafruit nRF52 bootloader stay in UF2 mode
/// after a reset: `DFU_MAGIC_UF2_RESET` in src/main.c of
/// github.com/adafruit/Adafruit_nRF52_Bootloader (read at commit ab17aef),
/// which checks `NRF_POWER->GPREGRET` at start and clears it.
const BOOTLOADER_UF2: u8 = 0x57;

fn reboot(bootloader: bool) -> ! {
    if bootloader {
        pac::POWER.gpregret().write(|w| w.set_gpregret(BOOTLOADER_UF2));
    }
    cortex_m::peripheral::SCB::sys_reset()
}
