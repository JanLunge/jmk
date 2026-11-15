//! Internal flash for the saved configuration and the Bluetooth bonds.
//!
//! While the radio is in use, flash may only be written in time slots the
//! radio scheduler (MPSL) hands out, which makes writing asynchronous. The
//! keyboard logic saves synchronously, so [`Flash`] waits in place.

use core::cell::RefCell;
use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use embassy_nrf::peripherals::NVMC;
use embassy_nrf::Peri;
use embedded_storage::nor_flash::{ErrorType, NorFlash, ReadNorFlash};
use jmk_device::STORAGE_SIZE;
use nrf_mpsl::{FlashError, MultiprotocolServiceLayer};

const PAGE: u32 = 4096;
/// Where the firmware image ends; must match FLASH in memory.x. The
/// bootloader starts at 0xF4000 and keeps the 40 KiB below it across
/// firmware updates (README.md, "Memory map").
pub const CONFIG_OFFSET: u32 = 0x000E_A000;
pub const BONDS_OFFSET: u32 = CONFIG_OFFSET + STORAGE_SIZE;
pub const BONDS_SIZE: u32 = 2 * PAGE;
const BOOTLOADER: u32 = 0x000F_4000;

const _: () = assert!(BONDS_OFFSET + BONDS_SIZE == BOOTLOADER && BONDS_OFFSET % PAGE == 0);

pub struct Flash(RefCell<nrf_mpsl::Flash<'static>>);

impl Flash {
    pub fn new(mpsl: &'static MultiprotocolServiceLayer<'static>, nvmc: Peri<'static, NVMC>) -> Self {
        Self(RefCell::new(nrf_mpsl::Flash::take(mpsl, nvmc)))
    }
}

/// Run a flash operation to its end without returning to the executor.
///
/// Time slots are granted in MPSL's own interrupts, but only after its
/// low-priority processing has run. That normally happens in a task of the
/// executor this call is blocking, so it is done here instead.
fn block_on<T>(operation: impl Future<Output = T>) -> T {
    let mut operation = pin!(operation);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(result) = operation.as_mut().poll(&mut context) {
            return result;
        }
        // SAFETY: MPSL is initialised, since a `Flash` exists. This must not
        // run concurrently with itself; the only other caller is the MPSL
        // task on the same thread-mode executor, which cannot run while this
        // loop does.
        unsafe { nrf_mpsl::raw::mpsl_low_priority_process() };
    }
}

impl ErrorType for &Flash {
    type Error = FlashError;
}

impl ReadNorFlash for &Flash {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), FlashError> {
        self.0.borrow_mut().read(offset, bytes)
    }

    fn capacity(&self) -> usize {
        embassy_nrf::nvmc::FLASH_SIZE
    }
}

impl NorFlash for &Flash {
    const WRITE_SIZE: usize = 4;
    const ERASE_SIZE: usize = PAGE as usize;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        block_on(self.0.borrow_mut().erase(from, to))
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), FlashError> {
        block_on(self.0.borrow_mut().write(offset, bytes))
    }
}
