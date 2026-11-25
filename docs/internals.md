# How JMK is put together

Notes for working on the firmware or porting it. For what the project is and
how to run it, see the [README](../README.md).

## What lives where

| Path | What it is |
| --- | --- |
| `crates/jmk-core` | Keymap engine. No hardware knowledge. |
| `crates/jmk-proto` | Configuration data model and the wire protocol. |
| `crates/jmk-device` | Scanning, debouncing, wiring detection, split link, flash storage and request handling, written against small pin, flash and link traits. |
| `crates/jmk-sim` | Simulated wiring, flash and cable; virtual keyboards. |
| `crates/jmk-wasm` | The above compiled for the configurator. |
| `crates/jmk-embassy` | What the ports share: USB interfaces, queues, main loop, split cable. |
| `firmware/rp2040` | The RP2040 port (Embassy). |
| `firmware/nrf52840` | The nRF52840 port with Bluetooth. Experimental. |
| `app/` | POG, the configurator. See its README. |
| `tools/cli` | `jmk`, a command-line client for the protocol (Linux). |
| `tools/uf2` | Turns a firmware ELF file into a UF2 image. |

A port for a new chip implements `jmk_device::Pins`, supplies a flash driver
and connects the transports. Everything else is shared.

## Pin safety

The firmware starts with no pins in use and touches none until told to. It
only ever drives a pin low, and only pins named in the hardware
configuration or offered to the wiring detection. Pins marked as protected
(display, LEDs, power) are refused for both. Detection uses the internal
weak pull resistors, so two probed pins cannot drive against each other.
The light chain's data pin is the one exception: the firmware drives it
high and low, and only the pin the user names for it; that pin is refused
for keys and for detection.
Finding the cable between two halves works the same way: one half holds a
pin low while the other half's detection watches, and the app first rules out
pins tied to supply voltage.

A cable usually has a data wire for each direction, on pins a UART can use.
One with a single data wire works too, on the RP2040 and on any pin: the
halves take turns on it (`LinkConfig::Wire`, `wire` in
`firmware/rp2040/src/link.rs`). Nobody drives that wire high, so two halves
talking at once garble a message but cannot harm the pins; each waits for
the wire to be quiet for a random while before it talks.

## Wiring the firmware can read

All of this is set at run time; nothing is compiled in.

- One pin per key, to ground or to supply voltage.
- A row/column matrix, with diodes in either direction or without diodes.
- A duplex matrix: two keys per crossing with their diodes pointing opposite
  ways, read once in each direction. Like any duplex matrix it can show a key
  that is not held when four or more are down.
- Any mix of the above on one board, up to four groups, as long as no pin is
  in two of them. The groups take their key positions one after the other.
- Rotary knobs, two positions each, after the switches.

A press counts at once and the key is then ignored for the debounce time. A
release counts once the key has read open for the whole debounce time
(5 ms unless changed).

## Build and test

```sh
./scripts/check.sh    # all tests, firmware build, app tests
```

There is no hosted CI; this script is the gate. It needs the Rust targets
`wasm32-unknown-unknown`, `thumbv6m-none-eabi` and `thumbv7em-none-eabihf`,
Node.js, and libclang for the nRF52840 build. It leaves the firmware images
in `target/` (`jmk-rp2040.uf2`, `jmk-nrf52840.uf2`). Hold BOOTSEL while
plugging the board in and copy the file to the drive that appears. A board
already running JMK is updated
with `cargo run -p jmk-cli -- flash target/jmk-rp2040.uf2` or from the app.
The image assumes 2 MB of flash and keeps the saved configuration in the
last 32 KiB.

## Protocol stability

`Request::GetInfo` and `Request::Reboot` keep their wire positions for ever,
so any configurator can identify and update any firmware. Everything else
may still change until the first release; new requests go at the end.

## Firmware updates

The app carries one firmware file per chip (`npm run firmware` copies them
from `target/`) and knows their version from `FIRMWARE_VERSION` in
`jmk-proto`, which the firmware reports in `GetInfo`. A keyboard that reports
an older one is offered the update: the app saves, asks the board to restart
into its UF2 bootloader, copies the file onto the drive that appears and
waits for the keyboard to come back, one half of a split after the other.

Count `FIRMWARE_VERSION` up with every firmware change a keyboard should
get. Without that the app has no way to tell and offers nothing.
