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
use crate::wiring::Wiring;

/// Run the keyboard forever.
///
/// `feed_watchdog` is called on every pass, so a watchdog started by the
/// port resets the chip if the loop stops and a hang never needs a hand on
/// the board. Saving to flash blocks one pass for a few hundred
/// milliseconds; the timeout must allow for that. `reboot` gets true for the
/// bootloader. `host` says whether a computer is listening right now; of two
/// connected halves the one with a computer types.
///
/// The port's tasks for the cable and the lights must run next to this one
/// in the same `join`, and follow `wiring` as described there.
pub async fn run<P: Pins, F: NorFlash>(
    device: &mut Device<'_, P, F>,
    mut out: impl Output,
    mut link: impl Link,
    config: &ConfigQueues,
    wiring: &Wiring,
    mut feed_watchdog: impl FnMut(),
    host: impl Fn() -> bool,
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
    wiring.set_link(device.hardware().split.link());
    wiring.set_light(device.lighting().pin);
    let mut wired = (device.hardware().split.link(), device.lighting().pin);
    loop {
        feed_watchdog();
        // If a pass overruns its millisecond the ticker is ready again at
        // once; yielding keeps USB and the other futures running.
        yield_now().await;
        match select(ticker.next(), config.from_host.receive()).await {
            Either::First(()) => {
                let now = Instant::now();
                device.set_host(host());
                device.set_locks(crate::usb::locks());
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
                let now = (device.hardware().split.link(), device.lighting().pin);
                if now != wired {
                    if now.0 != wired.0 {
                        wiring.set_link(now.0);
                    }
                    if now.1 != wired.1 {
                        wiring.set_light(now.1);
                    }
                    wired = now;
                    // The drivers let go of their old pins during this
                    // pause. One of them may be wired to a key now.
                    yield_now().await;
                    device.claim_pins();
                }
                if let Some(bootloader) = device.take_reboot() {
                    // Give the reply time to reach the host first.
                    reboot_at = Some((Instant::now() + Duration::from_millis(100), bootloader));
                }
            }
        }
    }
}
