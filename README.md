<img src="assets/icon.svg" width="64" height="64" alt="">

# JMK

Keyboard firmware in Rust that is configured after flashing, not before
compiling. One prebuilt image per microcontroller family; pins, matrix
type, keymap and behaviours are set from a configurator app and stored on
the keyboard. The aim is that someone with a hand-wired board flashes a file,
clicks through a setup wizard that finds the wiring by asking them to press
each key, and is done.

## What works today

- **Firmware for RP2040** (Raspberry Pi Pico and similar), run on a real
  Pico: USB keyboard and media keys, configuration channel, saving to flash,
  self-update from the app or command line.
- **Wiring**: diode matrix in either direction, one pin per key, rotary
  knobs. Pins are assigned at run time.
- **Keymap engine**: layers, hold-tap in three flavours, combos, one-shot
  modifiers, macros.
- **Split keyboards** over a serial cable, with the keymap on the half that
  plugs into the computer.
- **Configurator** (`app/`): setup wizard that finds the wiring from key
  presses, layout editor, keymap editor, hold-tap, combos, macros, firmware
  update, keyboard files, registry browser. Includes virtual keyboards that
  run the real firmware logic, for trying it without hardware.
- **Registry server** (`server/`) for sharing keyboard definitions and keymaps.

See [Status](#status) for what has been tested where.

## Layout

| Path | What it is |
| --- | --- |
| `crates/jmk-core` | Keymap engine. No hardware knowledge. |
| `crates/jmk-proto` | Configuration data model and the wire protocol. |
| `crates/jmk-device` | Scanning, wiring detection, split link, flash storage and request handling, written against small pin, flash and link traits. |
| `crates/jmk-sim` | Simulated wiring, flash and cable; virtual keyboards. |
| `crates/jmk-wasm` | The above compiled for the configurator. |
| `firmware/rp2040` | The RP2040 port (Embassy). |
| `app/` | The configurator. See its README. |
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

## Build and test

```sh
./scripts/check.sh    # all tests, firmware build, app tests
```

There is no hosted CI; this script is the gate. It leaves the firmware image
at `target/jmk-rp2040.uf2`. Hold BOOTSEL while plugging the board in and copy
the file to the drive that appears. A board already running JMK is updated
with `cargo run -p jmk-cli -- flash target/jmk-rp2040.uf2` or from the app.
The image assumes 2 MB of flash and keeps the saved configuration in the
last 32 KiB.

## Protocol stability

`Request::GetInfo` and `Request::Reboot` keep their wire positions for ever,
so any configurator can identify and update any firmware. Everything else
may still change until the first release; new requests go at the end.

## Status

**Checked on a real Raspberry Pi Pico (6-key diode matrix):**
USB enumeration with keyboard, media and configuration interfaces; the
protocol; matrix scanning and the keyboard report of a held key; wiring
detection finding that key's two pins; saving and loading across restarts;
firmware update without touching the board; the desktop app reading the
keyboard over USB.

**Checked only in simulation** (the same code, on simulated wiring):
key release and debounce on real switches; hold-tap and combo timing by
hand; macros and media keys reaching a computer; rotary knobs; the serial
link between split halves; one-pin-per-key wiring; the wizard's detection
on real diodeless or miswired boards; the watchdog recovering a hang.

**Written but never run:** copying firmware to a bootloader drive from the
desktop app; the app in Chrome on Windows and macOS; desktop installers.

**Not built yet:**
- Bluetooth, and split halves over Bluetooth.
- Displays, RGB lighting, pointing devices, mouse keys, tap dance, N-key
  rollover (reports carry six keys).
- Mixing matrix and one-pin-per-key wiring on one board; shift registers,
  I/O expanders and optical switches (Svalboard).
- Either half of a split plugging into the computer; today it is always
  the same one.
- Plugins beyond the `Custom` action hook.
- A USB product ID of our own (the firmware uses the pid.codes test ID).
- Controller pictures other than the Raspberry Pi Pico are unverified.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Contributions are accepted under
the same terms.
