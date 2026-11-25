//! The serial cable between two halves of a split keyboard.
//!
//! The RP2040 can only put a UART on certain pins, so the pin pair from the
//! configuration selects the UART. A changed link configuration is
//! followed while the keyboard runs.

use embassy_futures::select::{select, Either};
use embassy_rp::bind_interrupts;
use embassy_rp::pac;
use embassy_rp::peripherals::{self, UART0, UART1};
use embassy_rp::uart::{BufferedInterruptHandler, BufferedUart, Config, TxPin};
use embassy_rp::Peri;
use jmk_embassy::link::{Queues, BAUD};
use jmk_embassy::wiring::Wiring;
use jmk_proto::{LinkConfig, Pin};

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

/// Run the cable on whichever pins are configured at the time, forever.
/// Nothing else may use the UARTs, or those pins.
pub async fn run(wiring: &Wiring, queues: &Queues) -> ! {
    let mut bufs = ([0u8; 128], [0u8; 128]);
    let mut link = wiring.link().await;
    loop {
        let cable = async {
            match link.and_then(|link| open(&link, &mut bufs)) {
                Some(uart) => {
                    let (tx, rx) = uart.split();
                    jmk_embassy::link::run(tx, rx, queues).await
                }
                None => core::future::pending().await,
            }
        };
        // Ends with a new setting; the UART is dropped with `cable`.
        let next = match select(cable, wiring.link()).await {
            Either::First(()) => link,
            Either::Second(next) => next,
        };
        // The driver leaves its pins on the UART. Take them off, or a UART
        // that moves to other pins keeps listening on the old ones too.
        if let Some(LinkConfig::Uart { tx, rx }) = link {
            for pin in [tx, rx] {
                pac::IO_BANK0.gpio(pin as usize).ctrl().write(|w| w.set_funcsel(pac::io::vals::Gpio0ctrlFuncsel::NULL as _));
            }
        }
        link = next;
    }
}

/// Claims the UART and the two pins. The UART returned before must be gone.
fn open(link: &LinkConfig, bufs: &mut ([u8; 128], [u8; 128])) -> Option<BufferedUart> {
    let LinkConfig::Uart { tx, rx } = *link else { return None };
    if !supported(link) {
        return None;
    }
    let mut config = Config::default();
    config.baudrate = BAUD;

    // The driver wants pin types, the configuration has numbers: these
    // spell out the pins each UART can use.
    macro_rules! open {
        ($uart:ident, tx: [$($txn:literal $txp:ident),*], rx: [$($rxn:literal $rxp:ident),*]) => {{
            fn with_tx(
                tx: Peri<'static, impl TxPin<$uart>>,
                rx: Pin,
                bufs: &mut ([u8; 128], [u8; 128]),
                config: Config,
            ) -> Option<BufferedUart> {
                // SAFETY: see `open`; each arm claims the UART and one pin once.
                let uart = unsafe { peripherals::$uart::steal() };
                match rx {
                    $($rxn => Some(BufferedUart::new(
                        uart, tx, unsafe { peripherals::$rxp::steal() }, Irqs, &mut bufs.0, &mut bufs.1, config,
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
    if UART0_TX.contains(&tx) {
        open!(UART0, tx: [0 PIN_0, 12 PIN_12, 16 PIN_16, 28 PIN_28], rx: [1 PIN_1, 13 PIN_13, 17 PIN_17, 29 PIN_29])
    } else {
        open!(UART1, tx: [4 PIN_4, 8 PIN_8, 20 PIN_20, 24 PIN_24], rx: [5 PIN_5, 9 PIN_9, 21 PIN_21, 25 PIN_25])
    }
}
