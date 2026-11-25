//! The serial cable between two halves of a split keyboard.
//!
//! The nRF52840 can route its UART to any pin, so the pin pair from the
//! configuration is used as it is. A changed link configuration is
//! followed while the keyboard runs.

use embassy_futures::select::{select, Either};
use embassy_nrf::buffered_uarte::{self, BufferedUarte};
use embassy_nrf::gpio::AnyPin;
use embassy_nrf::peripherals::{PPI_CH0, PPI_CH1, PPI_GROUP0, TIMER1, UARTE0};
use embassy_nrf::{bind_interrupts, uarte};
use jmk_embassy::link::{Queues, BAUD};
use jmk_embassy::wiring::Wiring;
use jmk_proto::LinkConfig;

bind_interrupts!(struct Irqs {
    UARTE0 => buffered_uarte::InterruptHandler<UARTE0>;
});

const _: () = assert!(BAUD == 115_200);

pub fn supported(link: &LinkConfig) -> bool {
    match link {
        // Whether the pins exist and are free is checked with all others.
        LinkConfig::Uart { .. } => true,
        // Bluetooth between the halves is not implemented, and this
        // chip's UART cannot send and listen on one pin.
        LinkConfig::Ble | LinkConfig::Wire { .. } => false,
    }
}

/// Run the cable on whichever pins are configured at the time, forever.
/// Nothing else may use UARTE0, TIMER1, PPI channels 0 and 1 or PPI group 0,
/// and `NrfPins` leaves the link pins alone.
pub async fn run(wiring: &Wiring, queues: &Queues) -> ! {
    let mut bufs = ([0u8; 128], [0u8; 128]);
    let mut link = wiring.link().await;
    loop {
        let cable = async {
            match link.and_then(|link| open(&link, &mut bufs)) {
                Some(uart) => {
                    let (rx, tx) = uart.split();
                    jmk_embassy::link::run(tx, rx, queues).await
                }
                None => core::future::pending().await,
            }
        };
        // Ends with a new setting. The UART is dropped with `cable`, and
        // that disconnects its pins.
        if let Either::Second(next) = select(cable, wiring.link()).await {
            link = next;
        }
    }
}

/// The pins must have passed the hardware configuration's checks.
fn open<'d>(link: &LinkConfig, bufs: &'d mut ([u8; 128], [u8; 128])) -> Option<BufferedUarte<'d>> {
    let LinkConfig::Uart { tx, rx } = *link else { return None };
    let mut config = uarte::Config::default();
    config.baudrate = uarte::Baudrate::Baud115200;
    // SAFETY: see `run`; the UART opened before is gone, as it borrowed `bufs`.
    let uart = unsafe {
        BufferedUarte::new(
            UARTE0::steal(),
            TIMER1::steal(),
            PPI_CH0::steal(),
            PPI_CH1::steal(),
            PPI_GROUP0::steal(),
            AnyPin::steal(rx),
            AnyPin::steal(tx),
            Irqs,
            config,
            &mut bufs.0,
            &mut bufs.1,
        )
    };
    Some(uart)
}
