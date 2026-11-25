<p align="center"><img src="assets/icon.svg" width="96" height="96" alt=""></p>
<h1 align="center">JMK + POG v2</h1>
<h4 align="center">
Keyboard firmware you configure after flashing, and the app that does it
</h4>

> **Built with a lot of AI help.** I design this, decide what goes in and test
> it on my own keyboards, but most of the code was written with AI coding
> tools (Claude) and not typed by hand. If that is not for you, better to know
> now.

![The setup finding a macro pad's wiring from key presses](assets/wizard.png)

**JMK** is keyboard firmware in Rust. You flash one prebuilt file, and pins,
wiring, keymap and everything else are set from the app afterwards and kept
on the keyboard. No config files, no compiling.

**POG v2** is that app (`app/`). It is the follow-up to
[POG](https://github.com/JanLunge/pog), which did the same job for KMK.

The idea: flash a hand-wired board, press every key once so the app can work
out how it is wired, and start typing.

# Status

Early alpha. No releases or installers yet, and the protocol and the saved
format can still change. I use it on macOS and Ubuntu; Windows is untried.

What has actually run on hardware:

| | |
| --- | --- |
| RP2040 (Pi Pico and friends) | Runs on my own boards. This is the one to use. |
| nRF52840 (nice!nano and friends) | Compiles. Has never run on a board. |
| Split keyboards over a cable | Experimental. Mostly tried on the virtual keyboards. |
| Duplex matrix, mixed wiring | New. Only tried on the virtual keyboards. |
| RGB lights (WS2812) | RP2040 only. |

# What it does

- finds the wiring from key presses: one pin per key, a matrix with or
  without diodes, a duplex matrix, or a mix of them on one board
- rotary knobs, split keyboards over a serial cable, RGB underglow and
  per-key lights
- layers, hold-tap, combos, one-shot modifiers, macros you can record
- keymap editor: click or drag keys, or just type on your normal keyboard to
  fill in key after key
- puts QWERTY, Colemak, Colemak-DH or Dvorak on the letter keys in one go
- layout editor for where the keys sit, with keyboard-layout-editor import
- every change is saved to the keyboard by itself, with undo and redo
- one file holds the whole keyboard (wiring, layout, keymap, lights) to back
  it up or set up the next one
- firmware update from the app in one click, no reset button, both halves of a split
- virtual keyboards that run the real firmware logic, so you can try all of
  it without hardware

# Try it

You need [Rust](https://rustup.rs) and Node.js.

```sh
rustup target add wasm32-unknown-unknown
cd app
npm install
npm run start
```

Pick a virtual keyboard on the first screen, or plug in a board.

For a real Pi Pico build the firmware first, then hold BOOTSEL while
plugging it in. The app offers to install it.

```sh
rustup target add thumbv6m-none-eabi
(cd firmware/rp2040 && cargo build --release)
cargo run -p jmk-uf2 -- rp2040 \
    firmware/rp2040/target/thumbv6m-none-eabi/release/jmk-rp2040 target/jmk-rp2040.uf2
(cd app && npm run firmware)
```

`./scripts/check.sh` runs every test and builds both firmwares. There is no
hosted CI yet, that script is it.

How the crates fit together, the pin safety rules and the protocol notes are
in [docs/internals.md](docs/internals.md).

# Tasks

## bugs and rough edges
- [ ] the keymap picture cannot be zoomed, big boards get small
- [ ] a duplex matrix can show a key that is not held with four or more keys down (true for the wiring itself, but untested on a real one)
- [ ] setting up a split with only one half plugged in restores only that half from a keyboard file
- [ ] the macro recorder cannot see shortcuts the system takes for itself

## next
- [ ] releases with installers for macOS, Windows and Linux
- [ ] CI that runs the check script
- [ ] run the nRF52840 port on a real board
- [ ] test split, duplex and mixed wiring on real boards
- [ ] Windows

## wishlist
- [ ] split halves over Bluetooth
- [ ] more than one Bluetooth host, and a pairing mode
- [ ] tap dance
- [ ] key labels for German and other host layouts
- [ ] a wiring preview while soldering
- [ ] a registry for sharing keyboards and keymaps (was started, taken out until it works)

# License

[Apache License 2.0](LICENSE).
