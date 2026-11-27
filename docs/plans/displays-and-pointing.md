# Plan: displays and pointing devices

Not started. Written down on 2026-11-27 so the reasoning is not lost.

## Goal

Support OLED screens, memory and e-paper displays, and pointing devices
(trackballs, trackpads, joysticks) the way JMK does everything else: one
prebuilt firmware, and the app says which part sits on which pins. Nobody
compiles anything or writes code for what a screen shows.

## Does it fit?

Yes, on both chips. Measured from release builds on 2026-11-27 (commit
`53b5ec0`):

|                        | RP2040 (2 MB boards) | nRF52840  |
| ---------------------- | -------------------- | --------- |
| Flash for firmware     | 2016 KiB             | 932 KiB   |
| Used                   | 112 KiB              | 251 KiB   |
| RAM                    | 264 KiB              | 256 KiB   |
| Fixed RAM use          | 42 KiB               | 60 KiB    |

The stack's peak is not measured.

Estimated cost of everything below, not measured: 40 to 75 KiB of flash and
under 10 KiB of RAM. So one firmware can carry every driver; there is no need
for builds per feature.

## What limits it instead

- **Saved setup is 32 KiB on both chips.** See "Storage" below.
- **Battery.** An OLED or an optical sensor draws milliamps; a sleeping
  keyboard draws microamps. Displays need a sleep setting from the start.
- **Pins on the RP2040.** Its hardware I2C and SPI only reach certain pins.
  Either the app restricts the choice, or the firmware uses PIO.
- **The split link.** Whether pointer movement fits over the one-wire link is
  unchecked.
- **The 1 ms key scan must never wait for a screen.** Drawing runs in the
  background, at a low rate, and only when something changed.

## Groundwork

1. I2C and SPI on pins chosen after flashing, in the pin layer, as the light
   chain already does.
2. A list of extra parts in the saved setup, kept apart from the wiring (as
   `MoreWiring` is) so older firmware and saved files still read.
3. Detection: an I2C scan that reports "a display at 0x3C on these two pins".
   SPI parts cannot be found and need their pins named.

## Pointing devices (first)

1. A mouse report on USB and Bluetooth, and mouse keys (move, scroll, click)
   as key actions. No hardware needed; everything else builds on it.
2. A small fixed set of sensors: PMW3360 and PMW3610 trackballs, the Cirque
   trackpad, an analog joystick.
3. In the app: move the ball and watch a pointer in a preview; set rotation,
   speed, and "this layer turns movement into scrolling".

## Displays

One display model, several drivers.

- Content is not code. The app offers widgets (layer name, held modifiers,
  battery, a picture) placed on a preview of the screen; the firmware draws
  them.
- Drivers in this order: SSD1306 and SH1106 OLEDs (common, found by I2C scan),
  the Sharp memory display of the nice!view (what most people mean by "e-ink"
  on a keyboard), then real e-paper, which refreshes slowly and needs its own
  update rules.

## Storage

The 32 KiB for the saved setup is the tight spot once screens hold custom
content. Two ideas, to be decided when this starts:

- **Make the area as large as it can be.** There are about 1900 KiB free on
  the RP2040 and about 680 KiB on the nRF52840. On the nRF52840 the area sits
  at a fixed address below the Bluetooth bonds, so growing it means moving the
  layout, and existing keyboards need a way across.
- **Store how to draw, not the drawing.** A screen layout, and even an
  animation, saved as instructions (shapes, text, widgets, steps over time)
  is far smaller than frames of pixels and scales to any screen size. Pictures
  that really are pictures can still be stored as pixels, compressed.

Both together would allow animations and several screen layouts.

## Order

1. Mouse report and mouse keys.
2. I2C and SPI on chosen pins; the extra-parts list in the protocol.
3. One trackball sensor end to end, with its page in the app.
4. OLED with three or four widgets.
5. Larger storage and drawing instructions.
6. Sharp memory display, then e-paper.
