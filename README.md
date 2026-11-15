<img src="assets/icon.svg" width="64" height="64" alt="">

# JMK

Keyboard firmware in Rust that is configured after flashing, not before
compiling. One prebuilt image per microcontroller family; pins, matrix
type, keymap and behaviours are set from a configurator app and stored on
the keyboard. The aim is that someone with a hand-wired board flashes a file,
clicks through a setup wizard that finds the wiring by asking them to press
each key, and is done.

**Status: early.** The keymap engine, the configuration protocol and the
device logic exist and are tested on the PC. The RP2040 firmware runs on a
Raspberry Pi Pico: it enumerates, answers the configuration protocol, saves
to flash and reflashes itself from the command line. The nRF52840 firmware
(USB and Bluetooth) builds but has not run on hardware yet; see
[firmware/nrf52840/README.md](firmware/nrf52840/README.md). There is no
configurator app yet; `tools/cli` is the stand-in.

## Layout

| Path | What it is |
| --- | --- |
| `crates/jmk-core` | Keymap engine: layers, hold-tap, combos, one-shot modifiers. No hardware knowledge. |
| `crates/jmk-proto` | Configuration data model and the wire protocol shared with the configurator. |
| `crates/jmk-device` | Matrix scanning, wiring detection, flash storage and request handling, written against small pin and flash traits. |
| `crates/jmk-embassy` | What the Embassy-based ports share: the USB interfaces, the main loop and the split cable. |
| `firmware/rp2040` | The RP2040 port: Embassy, USB keyboard, and a vendor HID channel for the configurator. |
| `firmware/nrf52840` | The nRF52840 port: the same over USB, plus a Bluetooth keyboard. Untested on hardware. |
| `tools/uf2` | Turns a firmware ELF file into a UF2 image. |
| `tools/cli` | `jmk`, a command-line client for the configuration protocol (Linux). |

A port for a new chip implements `jmk_device::Pins`, supplies a flash driver
and connects the transports. Everything else is shared.

## Pin safety

The firmware starts with no pins in use and touches none until told to. It
only ever drives a pin low, and only pins named in the hardware
configuration or offered to the wiring detection. Pins marked as reserved
(display, LEDs, power) are refused for both. Detection uses the internal
weak pull resistors, so two probed pins cannot drive against each other.

## Build and test

```sh
cargo test --workspace                 # engine, protocol and simulated devices

rustup target add thumbv6m-none-eabi
cd firmware/rp2040 && cargo build --release && cd ../..
cargo run -p jmk-uf2 -- rp2040 \
    firmware/rp2040/target/thumbv6m-none-eabi/release/jmk-rp2040 jmk-rp2040.uf2
```

`./scripts/check.sh` runs all of this and builds the nRF52840 image as well.

Hold BOOTSEL while plugging the board in and copy `jmk-rp2040.uf2` to the
drive that appears. A board already running JMK is updated with
`cargo run -p jmk-cli -- flash jmk-rp2040.uf2`. The image assumes 2 MB of flash and keeps the saved
configuration in the last 32 KiB.

## Planned

- Configurator (Electron and Vue): board selection, reserved pins, wiring
  wizard, layout drawing, keymap editor, flashing.
- nRF52840: testing on hardware, multiple hosts, battery measurement,
  power saving.
- Split keyboards with Bluetooth between the halves.
- Macros, tap dance, mouse keys, consumer keys over USB, N-key rollover.
- More wiring types: shift registers, I/O expanders, optical switches
  (Svalboard), plus pointing devices, encoders, displays and RGB.
- Plugins as Rust crates, later sandboxed scripts that install without
  compiling.
- An online registry of keyboard definitions.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Contributions are accepted under
the same terms.
