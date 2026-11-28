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
| `firmware/rp2040` | The RP2040 port (Embassy). Its `src` is also the RP2350 port's. |
| `firmware/rp2350` | The RP2350 port: the target, the memory layout and the features that pick the chip. No code of its own. Not yet run on a board. |
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
One with a single data wire works too, on the RP2040 and RP2350 and on any pin: the
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
- Any mix of the above on one board, up to eight groups, as long as no pin is
  in two of them. The groups take their key positions one after the other.
  Keys that join two pins and share none with the rest (the push switch of a
  knob on two pins of its own, say) are a matrix of their own, of one row and
  one column if need be.
- Keys with and without a diode in one matrix. Read one way, a key without a
  diode is a key like the others, though ghosting can pass through it. In a
  duplex matrix it closes both positions of its crossing; the second is
  listed in `Doubles` and never reported, so it is one key.
- Rotary knobs, two positions each, after the switches.

A press counts at once and the key is then ignored for the debounce time. A
release counts once the key has read open for the whole debounce time
(5 ms unless changed).

## Build and test

```sh
./scripts/check.sh    # all tests, firmware build, app tests
```

There is no hosted CI; this script is the gate. It needs the Rust targets
`wasm32-unknown-unknown`, `thumbv6m-none-eabi`, `thumbv8m.main-none-eabihf`
and `thumbv7em-none-eabihf`,
Node.js, and libclang for the nRF52840 build. It leaves the firmware images
in `target/` (`jmk-rp2040.uf2`, `jmk-rp2350.uf2`, `jmk-nrf52840.uf2`). Hold BOOTSEL while
plugging the board in and copy the file to the drive that appears. A board
already running JMK is updated
with `cargo run -p jmk-cli -- flash target/jmk-rp2040.uf2` or from the app.
The image assumes 2 MB of flash and keeps the saved configuration in the
last 32 KiB.

## RP2040 and RP2350

Both are built from `firmware/rp2040/src`. `firmware/rp2350` is a package
without code: its `Cargo.toml` names that `src` as its program and turns on
the feature `rp235x`, which marks the few places where the chips differ:

- the chip the firmware reports, and where its serial number comes from
  (the flash chip on the RP2040, the chip itself on the RP2350);
- the pins a UART can use for the split cable: the RP2350 has more
  (`ports` in `link.rs`, and the same lists in `app/src/lib/cable.ts`);
- the name of the GPIO function in the pin registers.

What a build needs besides is in its own folder: the target
(`.cargo/config.toml`) and the memory layout (`memory.x`). An RP2350 has no
second-stage boot loader in its image; a small block after the vector table
tells the boot ROM what the image is, and `embassy-rp` supplies it.

The RP2350 image also uses 2 MB of flash only, with the saved configuration
in the last 32 KiB of those. That fits every RP2350: the RP2354 has 2 MB
built in, and on a board with a larger flash chip (the Pico 2 has 4 MB) the
rest stays unused. Its UF2 file carries the family `rp2350-arm-s`; the boot
ROM refuses the RP2040's.

The default feature `rp235xa` is for the package with 30 GPIOs (RP2350A,
RP2354A), and the image the app carries is that one. `--no-default-features
--features rp235xb` builds for the one with 48 (RP2350B, RP2354B). There a
cable with a single data wire is limited to GPIO 0 to 31, and the app does
not know that yet, nor does it carry that image.

## Protocol stability

`Request::GetInfo` and `Request::Reboot` keep their wire positions for ever,
so any configurator can identify and update any firmware. Nothing else that
was released moves either: new requests, answers and events go at the end,
and a struct that is sent or saved stays as it is.
`crates/jmk-sim/tests/wire.rs` checks that against the list in `wire.txt`
beside it.

Work on two branches at once can add to the same end and take the same
protocol version. The branch merged second moves its additions behind the
first one's and takes the next version. To avoid that, a branch can claim its
place first: a small change to `main` with the new requests (answered with
`Unsupported`), the version and the app's constant for it.

## Firmware updates

The app carries one firmware file per chip (`npm run firmware` copies them
from `target/`) and knows their version from `FIRMWARE_VERSION` in
`jmk-proto`, which the firmware reports in `GetInfo`. A keyboard that reports
an older one is offered the update: the app saves, asks the board to restart
into its UF2 bootloader, copies the file onto the drive that appears and
waits for the keyboard to come back, one half of a split after the other.

Count `FIRMWARE_VERSION` up with every firmware change a keyboard should
get. Without that the app has no way to tell and offers nothing.

## Linux

Linux gives a keyboard's settings channel (a hidraw device) to root alone, so
the app needs the udev rule in `app/linux/`. The desktop app installs it
through `pkexec` when it sees a keyboard it may not open.

- The rule opens only the third USB interface, the settings channel. A
  program that could read the first one would see every key pressed.
- It matches the maker's name `JMK`, not USB numbers, so it survives a change
  of those (see [usb-id.md](usb-id.md)).
- It relies on systemd-logind (`uaccess`). Without it, `GROUP="plugdev",
  MODE="0660"` in place of the tag does the same for that group.
- A browser installed as a snap or flatpak may be kept from USB keyboards by
  its own sandbox whatever the rule says.
- A firmware update copies a file onto the drive the board shows up as, which
  the desktop has to mount by itself, as GNOME and KDE do.
