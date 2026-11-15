//! The serial cable between two halves of a split keyboard.
//!
//! The nRF52840 can route its UART to any pin, so the pin pair from the
//! configuration is used as it is. The link is set up once at start; a
//! changed link configuration takes effect after a restart.

use embassy_nrf::buffered_uarte::{self, BufferedUarte};
use embassy_nrf::gpio::AnyPin;
use embassy_nrf::peripherals::{PPI_CH0, PPI_CH1, PPI_GROUP0, TIMER1, UARTE0};
use embassy_nrf::{bind_interrupts, uarte};
use jmk_embassy::link::{Queues, BAUD};
use jmk_proto::LinkConfig;
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    UARTE0 => buffered_uarte::InterruptHandler<UARTE0>;
});

const _: () = assert!(BAUD == 115_200);

pub fn supported(link: &LinkConfig) -> bool {
    match link {
        // Whether the pins exist and are free is checked with all others.
        LinkConfig::Uart { .. } => true,
        // Bluetooth between the halves is not implemented.
        LinkConfig::Ble => false,
    }
}

pub struct SerialLink(BufferedUarte<'static>);

impl SerialLink {
    /// Claims UARTE0, TIMER1, two PPI channels, a PPI group and the two
    /// pins. Must be called at most once, and nothing else may use any of
    /// them. The pins must have passed the hardware configuration's checks.
    pub fn open(link: &LinkConfig) -> Option<Self> {
        let LinkConfig::Uart { tx, rx } = *link else { return None };
        static TX_BUF: StaticCell<[u8; 128]> = StaticCell::new();
        static RX_BUF: StaticCell<[u8; 128]> = StaticCell::new();
        let mut config = uarte::Config::default();
        config.baudrate = uarte::Baudrate::Baud115200;
        // SAFETY: see above; this function is the only user of these
        // peripherals, and `NrfPins` leaves the link pins alone.
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
                RX_BUF.init([0; 128]),
                TX_BUF.init([0; 128]),
            )
        };
        Some(SerialLink(uart))
    }

    /// Move bytes between the cable and the queues, forever.
    pub async fn run(self, queues: &Queues) {
        let (rx, tx) = self.0.split();
        jmk_embassy::link::run(tx, rx, queues).await;
    }
}
