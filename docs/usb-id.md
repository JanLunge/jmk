# USB numbers of our own

Every USB device reports a vendor and a product number. JMK firmware reports
`1209:0001`, the test numbers of [pid.codes](https://pid.codes/1209/0001/).
Their page says they are for private testing and "MUST NOT be used on any
device that will be redistributed, sold, or manufactured". Keyboards other
people build and firmware files we hand out are past that, so JMK needs a
product number of its own before a release.

## Getting one

pid.codes hands out product numbers under vendor `1209` to open source
projects, free of charge, by pull request. What they ask for
([howto](https://pid.codes/howto/)):

- a public repository with the source, and a `LICENSE` file with a recognised
  open source licence. This repository has both (Apache 2.0).
- A project that is firmware only "may" be asked why it needs a number of its
  own. The answer here: JMK is the firmware on the device, whatever board it
  runs on, and the configurator finds keyboards by it.

The steps, in a fork of
[pidcodes/pidcodes.github.com](https://github.com/pidcodes/pidcodes.github.com):

1. `org/JanLunge/index.md` (or another name for the owner):

   ```
   ---
   layout: org
   title: Jan Lunge
   site: https://github.com/JanLunge
   ---
   Maker of the JMK keyboard firmware and its configurator, POG.
   ```

2. `1209/<number>/index.md`, with a number nobody has yet. Those starting
   with 0 and 1 are reserved; check the [list](https://pid.codes/1209/).

   ```
   ---
   layout: pid
   title: JMK Keyboard
   owner: JanLunge
   license: Apache-2.0
   site: https://github.com/JanLunge/jmk
   source: https://github.com/JanLunge/jmk
   ---
   A keyboard running JMK, firmware for hand-wired keyboards that is set up
   from the computer instead of compiled.
   ```

3. Open the pull request. Numbers go to whoever asks first; no review time
   is promised.

## When the number is there

- `crates/jmk-embassy/src/usb.rs`: `VID` and `PID`, and drop the comment
  about test numbers. Count `FIRMWARE_VERSION` up. The nRF52840 port
  reports the same two over Bluetooth and takes them from there.
- `app/electron/main.cjs`: add the new pair to `KEYBOARDS` and keep the old
  one, so keyboards on older firmware are still found and can be updated.

Nothing else knows the numbers. The browser finds keyboards by the firmware's
own HID usage page, the command-line tool by the same, and the Linux udev
rule by the maker's name `JMK`, so a rule installed today keeps working.
