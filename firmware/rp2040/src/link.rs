//! The serial cable between two halves of a split keyboard.
//!
//! A cable with a wire for each direction runs on a UART. The RP2040 can
//! only put one on certain pins, so the pin pair from the configuration
//! selects the UART. A cable with a single data wire runs on any pin: see
//! [`wire`]. A changed link configuration is followed while the keyboard
//! runs.

use embassy_futures::select::{select, Either};
use embassy_rp::bind_interrupts;
use embassy_rp::pac;
use embassy_rp::clocks::{clk_sys_freq, RoscRng};
use embassy_rp::gpio::{Level, Pull};
use embassy_rp::peripherals::{self, PIO1, UART0, UART1};
use embassy_rp::pio::program::{Assembler, InSource, JmpCondition, OutDestination, SetDestination, SideSet, WaitSource};
use embassy_rp::pio::{self, Direction, FifoJoin, Pio, ShiftConfig, ShiftDirection};
use embassy_rp::uart::{BufferedInterruptHandler, BufferedUart, Config, TxPin};
use embassy_rp::Peri;
use embassy_time::{with_timeout, Duration, Timer};
use fixed::types::U24F8;
use jmk_embassy::link::{Queues, BAUD};
use jmk_embassy::wiring::Wiring;
use jmk_proto::{LinkConfig, Pin};

bind_interrupts!(struct Irqs {
    UART0_IRQ => BufferedInterruptHandler<UART0>;
    UART1_IRQ => BufferedInterruptHandler<UART1>;
    PIO1_IRQ_0 => pio::InterruptHandler<PIO1>;
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
        LinkConfig::Wire { pin } => *pin < 30,
        LinkConfig::Ble => false,
    }
}

/// Run the cable on whichever pins are configured at the time, forever.
/// Nothing else may use the UARTs, this PIO, or those pins.
pub async fn run(wiring: &Wiring, queues: &Queues, mut pio: Peri<'static, PIO1>) -> ! {
    let mut bufs = ([0u8; 128], [0u8; 128]);
    let mut link = wiring.link().await;
    loop {
        let cable = async {
            if let Some(LinkConfig::Wire { pin }) = link {
                // Dropped with `cable`, which takes the pin off the PIO.
                return wire(pio.reborrow(), pin, queues).await;
            }
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

// --- One wire -----------------------------------------------------------------

/// Slower than the UART: the wire is only ever pulled up by the weak
/// resistors inside the two chips, and needs time to rise.
const WIRE_BAUD: u32 = 57_600;
const WIRE_BYTE_US: u64 = 10 * 1_000_000 / WIRE_BAUD as u64;
/// The wire must have been quiet this long before this half talks: longer
/// than a byte, so one on its way is noticed.
const WIRE_QUIET_US: u64 = 3 * WIRE_BYTE_US;

/// Serial in both directions over one wire, forever.
///
/// Two PIO programs share the pin. The sender never drives the wire high: a
/// zero pulls it to ground, a one lets go and the pull-ups raise it. Both
/// halves talking at once therefore garble the bytes but cannot short the
/// pins against each other. To keep that rare, a half talks only after the
/// wire was quiet for a while, a different while every time, so two halves
/// that collided do not collide again. A garbled message fails its checksum
/// and the state it carried is sent again anyway.
async fn wire(pio: Peri<'_, PIO1>, pin: Pin, queues: &Queues) {
    let Pio { mut common, sm0: mut sender, sm1: mut listener, .. } = Pio::new(pio, Irqs);

    // The driver wants pin types, the configuration has a number.
    macro_rules! claim {
        ($($number:literal $name:ident),*) => {
            match pin {
                // SAFETY: the keyboard logic never touches the cable pin
                // (it refuses a configuration that uses it twice).
                $($number => common.make_pio_pin(unsafe { peripherals::$name::steal() }),)*
                _ => return core::future::pending().await,
            }
        };
    }
    let mut line = claim!(
        0 PIN_0, 1 PIN_1, 2 PIN_2, 3 PIN_3, 4 PIN_4, 5 PIN_5, 6 PIN_6, 7 PIN_7, 8 PIN_8, 9 PIN_9,
        10 PIN_10, 11 PIN_11, 12 PIN_12, 13 PIN_13, 14 PIN_14, 15 PIN_15, 16 PIN_16, 17 PIN_17, 18 PIN_18,
        19 PIN_19, 20 PIN_20, 21 PIN_21, 22 PIN_22, 23 PIN_23, 24 PIN_24, 25 PIN_25, 26 PIN_26, 27 PIN_27,
        28 PIN_28, 29 PIN_29
    );
    line.set_pull(Pull::Up);
    // Eight PIO cycles to a bit, for both programs.
    let divider = U24F8::from_bits((clk_sys_freq() as u64 * 256 / (8 * WIRE_BAUD as u64)) as u32);

    // Sending. The pin's output is fixed at low and the program only
    // switches the pin between driving it and letting go, so it is handed
    // each byte inverted: a zero bit turns the driver on.
    let mut asm: Assembler<32> = Assembler::new_with_side_set(SideSet::new(true, 1, true));
    let mut bit = asm.label();
    // Stop bit, and where it waits for the next byte: let go.
    asm.pull_with_delay_and_side_set(false, true, 7, 0);
    // Start bit: pull low.
    asm.set_with_delay_and_side_set(SetDestination::X, 7, 7, 1);
    asm.bind(&mut bit);
    asm.out(OutDestination::PINDIRS, 1);
    asm.jmp_with_delay(JmpCondition::XDecNonZero, &mut bit, 6);
    let program = common.load_program(&asm.assemble_program());
    sender.set_pins(Level::Low, &[&line]);
    sender.set_pin_dirs(Direction::In, &[&line]);
    let mut config = pio::Config::default();
    config.set_out_pins(&[&line]);
    config.use_program(&program, &[&line]);
    config.clock_divider = divider;
    config.fifo_join = FifoJoin::TxOnly;
    config.shift_out = ShiftConfig { auto_fill: false, threshold: 32, direction: ShiftDirection::Right };
    sender.set_config(&config);
    sender.clear_fifos();
    sender.restart();
    sender.set_enable(true);

    // Listening: wait for a start bit, take eight bits from the middle of
    // each, and keep the byte only if the stop bit is there.
    let mut asm: Assembler<32> = Assembler::new();
    let mut start = asm.label();
    let mut bit = asm.label();
    let mut good = asm.label();
    asm.bind(&mut start);
    asm.wait(0, WaitSource::PIN, 0, false);
    asm.set_with_delay(SetDestination::X, 7, 10);
    asm.bind(&mut bit);
    asm.r#in(InSource::PINS, 1);
    asm.jmp_with_delay(JmpCondition::XDecNonZero, &mut bit, 6);
    asm.jmp(JmpCondition::PinHigh, &mut good);
    // No stop bit: garbled, or the wire is held low. Wait until it is let go.
    asm.wait(1, WaitSource::PIN, 0, false);
    asm.jmp(JmpCondition::Always, &mut start);
    asm.bind(&mut good);
    asm.r#in(InSource::NULL, 24);
    // Not waiting for room: if nobody collects, bytes are lost, not the timing.
    asm.push(false, false);
    let program = common.load_program(&asm.assemble_program());
    let mut config = pio::Config::default();
    config.use_program(&program, &[]);
    config.set_in_pins(&[&line]);
    config.set_jmp_pin(&line);
    config.clock_divider = divider;
    config.fifo_join = FifoJoin::RxOnly;
    config.shift_in = ShiftConfig { auto_fill: false, threshold: 32, direction: ShiftDirection::Right };
    listener.set_config(&config);
    listener.clear_fifos();
    listener.restart();
    listener.set_enable(true);

    let mut out = [0u8; 64];
    loop {
        // Until there is something to send, pass on what arrives.
        let len = loop {
            match select(queues.outgoing(&mut out), listener.rx().wait_pull()).await {
                Either::First(len) => break len,
                Either::Second(word) => queues.deliver(&[word as u8]),
            }
        };
        // Let the other half finish, and then some.
        loop {
            let quiet = WIRE_QUIET_US + 8 * RoscRng::next_u8() as u64;
            match with_timeout(Duration::from_micros(quiet), listener.rx().wait_pull()).await {
                Ok(word) => queues.deliver(&[word as u8]),
                Err(_) => break,
            }
        }
        for byte in &out[..len] {
            sender.tx().wait_push(!*byte as u32).await;
        }
        while !sender.tx().empty() {
            Timer::after_micros(WIRE_BYTE_US).await;
        }
        // The last byte is still on its way out.
        Timer::after_micros(2 * WIRE_BYTE_US).await;
        // The listener heard this half talk; none of that is news.
        while listener.rx().try_pull().is_some() {}
    }
}
