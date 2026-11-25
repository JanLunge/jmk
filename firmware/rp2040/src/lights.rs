//! The chain of RGB lights (WS2812 and compatible).
//!
//! A PIO program shapes the bits and DMA feeds it, so a picture goes out
//! with exact timing whatever the keyboard is busy with. A changed data pin
//! is followed while the keyboard runs.

use core::cell::RefCell;

use critical_section::Mutex;
use embassy_rp::clocks::clk_sys_freq;
use embassy_rp::peripherals::{self, DMA_CH0, PIO0};
use embassy_rp::pio::program::{Assembler, JmpCondition, OutDestination, SetDestination, SideSet};
use embassy_rp::pio::{self, Config, FifoJoin, Pio, ShiftConfig, ShiftDirection};
use embassy_futures::select::{select, Either};
use embassy_rp::{bind_interrupts, dma, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::Timer;
use fixed::types::U24F8;
use jmk_embassy::wiring::Wiring;
use jmk_proto::{Pin, MAX_LEDS};

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>;
});

// Lengths of the three parts of a bit, in PIO clock cycles.
const T1: u8 = 2;
const T2: u8 = 5;
const T3: u8 = 3;
const BITS_PER_SECOND: u32 = 800_000;

struct Picture {
    /// One word per light, its three bytes at the top in sending order.
    words: [u32; MAX_LEDS],
    len: usize,
}

static PICTURE: Mutex<RefCell<Picture>> = Mutex::new(RefCell::new(Picture { words: [0; MAX_LEDS], len: 0 }));
static FRESH: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Hand over the colours to send next, three bytes per light in the order
/// the chain wants them.
pub fn show(colors: &[[u8; 3]]) {
    critical_section::with(|cs| {
        let mut picture = PICTURE.borrow_ref_mut(cs);
        picture.len = colors.len().min(MAX_LEDS);
        for (word, color) in picture.words.iter_mut().zip(colors) {
            *word = u32::from_be_bytes([color[0], color[1], color[2], 0]);
        }
    });
    FRESH.signal(());
}

/// Send every picture handed to [`show`] down the chain, forever, on
/// whichever pin is configured at the time. Nothing else may use that pin.
pub async fn run(mut pio: Peri<'static, PIO0>, dma: Peri<'static, DMA_CH0>, wiring: &Wiring) -> ! {
    let mut channel = dma::Channel::new(dma, Irqs);
    let mut pin = wiring.light().await;
    loop {
        // Ends with a new pin. The PIO is dropped with it, and that takes
        // the old pin off the PIO at once.
        pin = match select(send(pio.reborrow(), &mut channel, pin), wiring.light()).await {
            Either::First(never) => match never {},
            Either::Second(pin) => pin,
        };
    }
}

async fn send(pio: Peri<'_, PIO0>, channel: &mut dma::Channel<'_>, pin: Option<Pin>) -> core::convert::Infallible {
    let Some(pin) = pin else { return core::future::pending().await };
    let Pio { mut common, mut sm0, .. } = Pio::new(pio, Irqs);

    // The driver wants pin types, the configuration has a number.
    macro_rules! claim {
        ($($number:literal $name:ident),*) => {
            match pin {
                // SAFETY: the keyboard logic never touches the light pin
                // (it refuses a configuration that uses it twice).
                $($number => common.make_pio_pin(unsafe { peripherals::$name::steal() }),)*
                _ => return core::future::pending().await,
            }
        };
    }
    let out = claim!(
        0 PIN_0, 1 PIN_1, 2 PIN_2, 3 PIN_3, 4 PIN_4, 5 PIN_5, 6 PIN_6, 7 PIN_7, 8 PIN_8, 9 PIN_9,
        10 PIN_10, 11 PIN_11, 12 PIN_12, 13 PIN_13, 14 PIN_14, 15 PIN_15, 16 PIN_16, 17 PIN_17, 18 PIN_18,
        19 PIN_19, 20 PIN_20, 21 PIN_21, 22 PIN_22, 23 PIN_23, 24 PIN_24, 25 PIN_25, 26 PIN_26, 27 PIN_27,
        28 PIN_28, 29 PIN_29
    );

    // One bit: high for T1, the data bit for T2, low for T3.
    let mut asm: Assembler<32> = Assembler::new_with_side_set(SideSet::new(false, 1, false));
    let mut wrap_target = asm.label();
    let mut wrap_source = asm.label();
    let mut zero = asm.label();
    asm.set_with_side_set(SetDestination::PINDIRS, 1, 0);
    asm.bind(&mut wrap_target);
    asm.out_with_delay_and_side_set(OutDestination::X, 1, T3 - 1, 0);
    asm.jmp_with_delay_and_side_set(JmpCondition::XIsZero, &mut zero, T1 - 1, 1);
    asm.jmp_with_delay_and_side_set(JmpCondition::Always, &mut wrap_target, T2 - 1, 1);
    asm.bind(&mut zero);
    asm.nop_with_delay_and_side_set(T2 - 1, 0);
    asm.bind(&mut wrap_source);
    let program = common.load_program(&asm.assemble_with_wrap(wrap_source, wrap_target));

    let mut config = Config::default();
    config.set_out_pins(&[&out]);
    config.set_set_pins(&[&out]);
    config.use_program(&program, &[&out]);
    let cycles_per_second = BITS_PER_SECOND / 1000 * (T1 + T2 + T3) as u32;
    config.clock_divider = U24F8::from_num(clk_sys_freq() / 1000) / U24F8::from_num(cycles_per_second);
    config.fifo_join = FifoJoin::TxOnly;
    config.shift_out = ShiftConfig { auto_fill: true, threshold: 24, direction: ShiftDirection::Left };
    sm0.set_config(&config);
    // Nothing half sent is left over from the pin before.
    sm0.clear_fifos();
    sm0.restart();
    sm0.set_enable(true);

    let mut words = [0u32; MAX_LEDS];
    // The picture of the moment goes out first: after a change of pin
    // nobody hands it over again.
    FRESH.signal(());
    loop {
        FRESH.wait().await;
        let len = critical_section::with(|cs| {
            let picture = PICTURE.borrow_ref(cs);
            words = picture.words;
            picture.len
        });
        sm0.tx().dma_push(channel, &words[..len], false).await;
        // The lights take the colours over once the line has been quiet.
        Timer::after_micros(300).await;
    }
}
