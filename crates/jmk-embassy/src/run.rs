//! The main loop: scan every millisecond, answer the configurator.

use embassy_futures::select::{select, Either};
use embassy_futures::yield_now;
use embassy_time::{Duration, Instant, Ticker};
use embedded_storage::nor_flash::NorFlash;
use jmk_core::Output;
use jmk_device::{Device, Link, Pins};
use jmk_proto::frame::{self, Assembler};
use jmk_proto::{DeviceMessage, Error, Request, Response};

use crate::queue::ConfigQueues;

/// Run the keyboard forever.
///
/// `feed_watchdog` is called on every pass, so a watchdog started by the
/// port resets the chip if the loop stops and a hang never needs a hand on
/// the board. Saving to flash blocks one pass for a few hundred
/// milliseconds; the timeout must allow for that. `reboot` gets true for the
/// bootloader.
pub async fn run<P: Pins, F: NorFlash>(
    device: &mut Device<'_, P, F>,
    mut out: impl Output,
    mut link: impl Link,
    config: &ConfigQueues,
    mut feed_watchdog: impl FnMut(),
    reboot: fn(bool) -> !,
) -> ! {
    let send = |message: &DeviceMessage| {
        // If the configurator is not reading, messages are dropped and it
        // has to ask again.
        let _ = frame::encode(message, |report| {
            let _ = config.to_host.try_send(*report);
        });
    };
    let mut ticker = Ticker::every(Duration::from_millis(1));
    let mut assembler = Assembler::new();
    let mut reboot_at: Option<(Instant, bool)> = None;
    loop {
        feed_watchdog();
        // If a pass overruns its millisecond the ticker is ready again at
        // once; yielding keeps USB and the other futures running.
        yield_now().await;
        match select(ticker.next(), config.from_host.receive()).await {
            Either::First(()) => {
                let now = Instant::now();
                device.poll(now.as_millis(), &mut out, &mut link, |event| send(&DeviceMessage::Event(event)));
                if let Some((at, bootloader)) = reboot_at {
                    if now >= at {
                        reboot(bootloader);
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
                    reboot_at = Some((Instant::now() + Duration::from_millis(100), bootloader));
                }
            }
        }
    }
}
