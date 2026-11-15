# JMK on the nRF52840

**Nothing in this crate has run on hardware.** It compiles and links, and it
is written against the sources of the libraries and the bootloader it relies
on, but no nRF52840 board was available. Treat every feature below as
unverified until the checklist at the end has been worked through.

## What is implemented

- `jmk_device::Pins` for all 48 GPIOs, claimed by number at run time. No pad
  is touched before the hardware configuration or the wiring detection asks
  for it.
- Configuration storage in internal flash, and a separate region for
  Bluetooth bonds.
- USB: the same three HID interfaces, descriptors and IDs as the RP2040 port
  (shared code in `crates/jmk-embassy`), the configurator channel, reboot
  and reboot to the UF2 bootloader.
- Bluetooth LE keyboard (HID over GATT) with `trouble-host` on Nordic's
  SoftDevice Controller (`nrf-sdc`, `nrf-mpsl`); no SoftDevice is used.
  HID service with keyboard and consumer reports, battery service, device
  information service, LE Secure Connections pairing ("Just Works"), one
  bond kept in flash, advertising whenever no host is connected.
- Output routing: key reports go over USB while a computer has the keyboard
  configured on the bus, otherwise over Bluetooth. When the route changes,
  everything is released on the one being left.
- Wired split: the same serial link as on the RP2040, on any two free pins
  (UARTE0, 115200 baud).
- A watchdog (10 s) that resets the chip if the main loop stops.

## What is not implemented

- Split halves over Bluetooth. `LinkConfig::Ble` is refused.
- More than one bonded host, and choosing between hosts. The bond table in
  flash has a slot per profile (`PROFILES` in `src/bonds.rs`), but there is
  one profile.
- A pairing mode. The keyboard advertises and accepts a pairing whenever it
  has no connection, and the newest pairing replaces the stored bond. Anyone
  in radio range can therefore pair while the keyboard's own host is away.
  There is also no way to delete the bond other than pairing again.
- Battery measurement. `src/battery.rs` is the one place the level comes
  from; it returns 100 % until measuring is configurable per board.
- Power saving. The matrix is scanned every millisecond, the 32 MHz crystal
  is kept running for USB at all times, and the DC/DC converters are off.
  Expect a battery to last days, not months.
- The configurator channel over Bluetooth, boot protocol over Bluetooth,
  showing the lock-key LEDs, passkey pairing.
- Flashing from the command line: `jmk flash` looks for the RP2040's
  bootloader drive only.
- Board power switches (for example the nice!nano's P0.13, which cuts
  power to external parts): mark such pins as reserved.

## Pin numbering

P0.n is pin n, P1.n is pin 32 + n, as in `jmk_proto::Pin`. For example
P0.06 is 6, P0.31 is 31, P1.00 is 32 and P1.15 is 47.

Never offered for wiring, on any board:

| Pin | Why |
| --- | --- |
| P0.00, P0.01 | 32.768 kHz crystal pins |
| P0.18 | reset (embassy-nrf configures it as the reset pin in UICR) |

P0.09 and P0.10 are the NFC antenna pins of the bare chip. Keyboard
controller boards wire them to the edge as ordinary pins, so the firmware is
built with embassy-nrf's `nfc-pins-as-gpio` and offers them. That setting
lives in UICR: on the first start the firmware clears the NFC bit there and
resets itself once. The bit stays cleared when other firmware is flashed
later.

Everything else a board uses for itself (LEDs, power switch, battery
divider) has to be marked as reserved in the hardware configuration.

## Memory map

| Address | Size | Content |
| --- | --- | --- |
| `0x00000000` | 4 KiB | MBR, left untouched |
| `0x00001000` | 932 KiB | this firmware |
| `0x000EA000` | 32 KiB | saved configuration |
| `0x000F2000` | 8 KiB | Bluetooth bonds |
| `0x000F4000` | 48 KiB | bootloader, MBR parameters, bootloader settings |

RAM starts at `0x20000008`: the MBR keeps the address it forwards interrupts
to in the first bytes of RAM. The firmware sets VTOR to its own vector table
at start (`set-vtor` in cortex-m-rt), so the MBR is not in the interrupt
path afterwards; the 8 bytes are left alone regardless.

### Why the firmware starts at 0x1000

The target is a board with the Adafruit nRF52 bootloader as shipped, for
example a nice!nano v2, which comes with SoftDevice S140 in flash. Sources
below are files in github.com/adafruit/Adafruit_nRF52_Bootloader, read at
commit `ab17aef`.

- The bootloader decides where the application starts each time it boots
  (`bootloader_app_start` in
  `lib/sdk11/components/libraries/bootloader_dfu/bootloader.c`): if a
  SoftDevice is present, directly after it, at the size the SoftDevice
  states about itself; otherwise at `0x1000`, directly after the MBR. A
  SoftDevice counts as present if the word at `0x3004` is its magic number
  (`is_sd_existed` in `dfu_types.h`).
- A UF2 file may write anywhere from `0x1000` up to `0xEA000`
  (`USER_FLASH_START`, `USER_FLASH_END` in `src/usb/uf2/uf2cfg.h`); "the
  SoftDevice is considered as part of application" (`write_block` in
  `src/usb/uf2/ghostfat.c`). Blocks addressed below `0x1000` are skipped,
  so the MBR cannot be overwritten this way, and it must not be: the
  bootloader is reached through it.
- After the last block the bootloader marks the application as valid in its
  settings page, with a CRC of zero, which means "do not check"
  (`tud_msc_write10_complete_cb` in `src/usb/msc_uf2.c`,
  `bootloader_app_is_valid` in `bootloader.c`). The image needs no marker,
  header or checksum of its own.
- Blocks must carry 256 bytes at an address that is a multiple of 256 and
  the family ID `0xADA52840` (`is_uf2_block`, `uf2cfg.h`). `tools/uf2`
  already writes exactly that.

This firmware does not use the SoftDevice, so there are two places it could
go. After the SoftDevice (`0x26000` for S140 6.1.1 on a nice!nano,
`0x27000` for S140 7.x on a XIAO BLE) would need one image per SoftDevice
version. At `0x1000` the image overwrites the SoftDevice, the magic number
disappears, and the bootloader starts the firmware at `0x1000` on every
board, whatever was installed before. JMK ships one image per chip, so it
starts at `0x1000`. RMK, which runs the same Bluetooth stack on the same
boards, uses the same layout (`examples/use_rust/nrf52840_ble/memory.x` in
github.com/HaoboGu/rmk).

The cost: firmware built to sit behind the SoftDevice (ZMK, CircuitPython,
Arduino cores) no longer starts after JMK has been flashed. Copying such a
UF2 file onto the board writes it at `0x26000`, but without a SoftDevice the
bootloader keeps starting whatever is at `0x1000`. To go back, put the
SoftDevice back first, either with the bootloader's update package
(`<board>_bootloader-<version>_s140_<version>.zip` from the bootloader's
release page, sent with `adafruit-nrfutil dfu serial`), or with a UF2 file
made from the SoftDevice hex file. The bootloader itself is never touched
and stays reachable with a double press of reset.

One theoretical trap: if a build happened to have the SoftDevice's magic
number `0x51B1E5DB` in the word at `0x3004`, the bootloader would take the
firmware for a SoftDevice. That word is part of the code and changes with
every build.

### The storage regions

The bootloader keeps the 40 KiB below itself for application data
(`DFU_APP_DATA_RESERVED`, set to `10*4096` for the nRF52840 in
`CMakeLists.txt`): UF2 files cannot write there, so the configuration and
the bond survive a firmware update. Other firmware uses the same region for
its own data. Both stores check a magic number and a CRC and start empty if
they find something else; saving then overwrites it.

While the radio is in use, flash may only be erased and written in time
slots handed out by the radio scheduler, in pieces of 10 ms. Saving the
configuration takes about 36 such slots, a factory reset about 72, and the
firmware waits in place for them. That is why the watchdog allows 10 s
where the RP2040 port allows 3 s.

## Low-frequency clock

A build-time choice, in `Cargo.toml` and `mod lfclk` in `src/main.rs`:

- default: the internal RC oscillator, calibrated by the radio scheduler.
  Works on every board.
- `--features lfxo`: a 32.768 kHz crystal on P0.00/P0.01. Uses less power.
  On a board without the crystal the firmware stops at start, waiting for
  the oscillator.

`scripts/check.sh` builds the default.

## Build and flash

Besides the Rust target, the build needs libclang with clang's own C
headers, because the Nordic libraries' bindings are generated at build
time. On Debian and Ubuntu that is the package `libclang-dev`.

```sh
rustup target add thumbv7em-none-eabihf
cd firmware/nrf52840 && cargo build --release && cd ../..
cargo run -p jmk-uf2 -- nrf52840 \
    firmware/nrf52840/target/thumbv7em-none-eabihf/release/jmk-nrf52840 jmk-nrf52840.uf2
```

Press reset twice quickly; a drive appears. Copy `jmk-nrf52840.uf2` to it.

`jmk flash` only knows the RP2040's bootloader drive. Run against an nRF
board it still sends the reboot-to-bootloader request, then fails waiting
for the wrong drive; the board's own drive is there and the file can be
copied by hand.

The Nordic SoftDevice Controller and MPSL libraries that get linked in are
closed source and have their own licence (see the `nrf-sdc` and `nrf-mpsl`
crates).

## Unverified until tested on hardware

In the order to test. Keep a second way into the bootloader at hand (double
press of reset) before starting.

1. **It starts.** Copy the UF2 file. The drive disappears and a USB device
   `1209:0001` "JMK Keyboard" appears; on the very first start there may be
   one or two extra resets while UICR is written (NFC pins, reset pin). If
   nothing appears: the start address, VTOR, the radio scheduler's start or
   the RC clock are wrong. This also tests that overwriting the SoftDevice
   makes the bootloader start the firmware at `0x1000`.
2. **It keeps running.** No re-enumeration for a minute. A reset every 10 s
   means the main loop is stuck and the watchdog fires.
3. **Configurator channel.** `jmk info` answers with `Nrf52840` and usable
   pins `0x0000_FFFF_FFFB_FFFC`. Three HID interfaces are listed (`lsusb -v`):
   boot keyboard, consumer control, vendor page `0xFF4A`.
4. **Pins.** Run the wiring detection and set a matrix or direct pins; keys
   type over USB. Check a pin on port 1 (number 32 or higher) and P0.09 or
   P0.10. Ghost keys or missed keys in a matrix point at the settle time in
   `src/pins.rs` (10 µs, counted in CPU cycles).
5. **Saving.** Save, unplug, plug in: the configuration is back. Then a
   factory reset. This is the first use of flash through the radio
   scheduler's time slots; a hang here ends in a watchdog reset after 10 s.
6. **Reboot to the bootloader.** Send a reboot request with
   `bootloader: true` (`jmk flash <file>` does, before it gives up waiting
   for an RP2040 drive): the UF2 drive appears and stays for longer than
   10 s. If it resets after 10 s, the installed bootloader version does not
   feed the watchdog the firmware left running.
7. **Update keeps the configuration.** After flashing a new UF2 file the
   saved configuration is still there.
8. **Without USB.** Power the board from a battery only: it starts (step 9
   shows that it does).
9. **Advertising.** A phone or computer sees "JMK Keyboard". If not, the
   Bluetooth controller refused to start (most likely `CONTROLLER_MEMORY`
   in `src/main.rs`) or the host stack stopped; USB keeps working in both
   cases, which is how to tell this from a crash.
10. **Pairing and typing.** Pair from Linux, Windows, macOS, Android and
    iOS in turn. Each should pair without a code, list a keyboard, and
    receive keys and media keys. Hosts differ in how strictly they read the
    HID service; note which ones fail.
11. **Reconnecting.** Switch the host's Bluetooth off and on; power-cycle
    the keyboard. Both must reconnect without pairing again (bond in flash,
    and for hosts with private addresses the resolving list).
12. **Pairing again.** Remove the keyboard on the host and pair again; pair
    a second host and confirm it replaced the first.
13. **Routing.** With a Bluetooth host connected, plug into a computer:
    keys go to USB only. Unplug: they go to Bluetooth again. Hold a key
    while plugging in; it must not stay stuck on the Bluetooth host after
    it is released.
14. **Saving while connected.** Save and factory-reset with a Bluetooth
    host connected. The connection should survive, and the save must finish
    well inside the watchdog's 10 s.
15. **Battery service.** The host shows 100 %.
16. **USB and Bluetooth together for hours.** The clock interrupt is shared
    between the radio scheduler and USB power detection; look for USB
    dropouts or a keyboard that stops reconnecting.
17. **Wired split.** Two halves with a serial link on freely chosen pins,
    with and without a Bluetooth host connected.
18. **Crystal build.** `--features lfxo` on a board that has the 32.768 kHz
    crystal.
