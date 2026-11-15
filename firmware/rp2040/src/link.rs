//! The serial cable between two halves of a split keyboard.
//!
//! The RP2040 can only put a UART on certain pins, so the pin pair from the
//! configuration selects the UART. The link is set up once at start; a
//! changed link configuration takes effect after a restart.

use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::{self, UART0, UART1};
use embassy_rp::uart::{BufferedInterruptHandler, BufferedUart, Config, TxPin};
use embassy_rp::Peri;
use jmk_embassy::link::{Queues, BAUD};
use jmk_proto::{LinkConfig, Pin};
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    UART0_IRQ => BufferedInterruptHandler<UART0>;
    UART1_IRQ => BufferedInterruptHandler<UART1>;
});

const UART0_TX: [Pin; 4] = [0, 12, 16, 28];
const UART0_RX: [Pin; 4] = [1, 13, 17, 29];
const UART1_TX: [Pin; 4] = [4, 8, 20, 24];
const UART1_RX: [Pin; 4] = [5, 9, 21, 25];

pub fn supported(link: &LinkConfig) -> bool {
    match link {
        LinkConfig::Uart { tx, rx } => {
            (UART0_TX.contains(tx) && UART0_RX.contains(rx)) || (UART1_TX.contains(tx) && UART1_RX.contains(rx))
        }
        LinkConfig::Ble => false,
    }
}

pub struct SerialLink(BufferedUart);

impl SerialLink {
    /// Claims the UART and the two pins. Must be called at most once, and
    /// nothing else may use these pins.
    pub fn open(link: &LinkConfig) -> Option<Self> {
        let LinkConfig::Uart { tx, rx } = *link else { return None };
        if !supported(link) {
            return None;
        }
        static TX_BUF: StaticCell<[u8; 128]> = StaticCell::new();
        static RX_BUF: StaticCell<[u8; 128]> = StaticCell::new();
        let bufs = (TX_BUF.init([0; 128]), RX_BUF.init([0; 128]));
        let mut config = Config::default();
        config.baudrate = BAUD;

        // The driver wants pin types, the configuration has numbers: these
        // spell out the pins each UART can use.
        macro_rules! open {
            ($uart:ident, tx: [$($txn:literal $txp:ident),*], rx: [$($rxn:literal $rxp:ident),*]) => {{
                fn with_tx(
                    tx: Peri<'static, impl TxPin<$uart>>,
                    rx: Pin,
                    bufs: (&'static mut [u8; 128], &'static mut [u8; 128]),
                    config: Config,
                ) -> Option<BufferedUart> {
                    // SAFETY: see `open`; each arm claims the UART and one pin once.
                    let uart = unsafe { peripherals::$uart::steal() };
                    match rx {
                        $($rxn => Some(BufferedUart::new(
                            uart, tx, unsafe { peripherals::$rxp::steal() }, Irqs, bufs.0, bufs.1, config,
                        )),)*
                        _ => None,
                    }
                }
                match tx {
                    $($txn => with_tx(unsafe { peripherals::$txp::steal() }, rx, bufs, config),)*
                    _ => None,
                }
            }};
        }
        let uart = if UART0_TX.contains(&tx) {
            open!(UART0, tx: [0 PIN_0, 12 PIN_12, 16 PIN_16, 28 PIN_28], rx: [1 PIN_1, 13 PIN_13, 17 PIN_17, 29 PIN_29])
        } else {
            open!(UART1, tx: [4 PIN_4, 8 PIN_8, 20 PIN_20, 24 PIN_24], rx: [5 PIN_5, 9 PIN_9, 21 PIN_21, 25 PIN_25])
        };
        uart.map(SerialLink)
    }

    /// Move bytes between the cable and the queues, forever.
    pub async fn run(self, queues: &Queues) {
        let (tx, rx) = self.0.split();
        jmk_embassy::link::run(tx, rx, queues).await;
    }
}
