# Plan: firmware updates over the cable

Status: idea, not started. Builds on settings over the cable (`Request::Peer`,
protocol 16, see [internals.md](internals.md)).

## Goal

A split keyboard with one USB cable gets new firmware on both halves from the
app in one click. Today the half without USB has to be plugged in to update,
because a chip's own bootloader (RP2040 boot ROM, nRF52840 UF2 bootloader)
only listens on USB.

## Why it's more than settings

Settings go over the cable as requests the far half already understands.
Firmware can't be handled the same way, for three reasons:

- **Size.** An RP2040 image is about 120 KB (240 KB as UF2). At today's pace
  of 24 bytes every 8 ms that takes about 40 seconds, which is acceptable but
  shouldn't get worse.
- **Nothing to fall back on.** A running firmware can't overwrite itself in
  place: the code that writes the flash runs from that same flash.
- **Recovery is awkward.** A half that doesn't start again has to be opened
  up for BOOTSEL. On a Svalboard that's a nuisance, so a failed update must
  leave the old firmware running.

## Design

### 1. Staging area

The far half writes the new image into a free area of its flash, away from
the running firmware and the saved settings.

- RP2040, 2 MB flash: firmware at the start; settings in the last 32 KiB.
  Use the 512 KiB below the settings as staging. That's far more than the
  image needs and leaves room for larger images later.
- nRF52840, 1 MB flash: the UF2 bootloader sits at the top, so check the
  layout before choosing an area.

### 2. Transfer

New requests, passed on with `Request::Peer` like any other:

- `BeginUpdate { size, crc32, chip, version }`: the far half checks that the
  chip matches and that the image fits, then erases the staging area. It
  answers once the erase is done, which takes time, so the asking half's
  `GIVE_UP_MS` must allow for it.
- `UpdateChunk { offset, data }`: data is written at `offset`. Each chunk is
  one request with its own reply, so a lost chunk is resent and a repeated
  one writes the same bytes again, which is harmless.
- `FinishUpdate`: the far half checks the CRC over the whole staging area and
  answers with the result. Nothing is replaced yet.
- `ApplyUpdate`: see step 3.

**Speed.** Raise the part size for this traffic: the frame's length byte
allows up to 255 bytes. Send several chunks before waiting for their replies,
with chunk offsets as sequence numbers. Aim for at least 10 KB/s on the
57,600 baud one-wire link, so an RP2040 image takes about 12 seconds. Typing
keeps working on the half on USB the whole time: key state frames keep their
priority.

### 3. Applying

The far half stops scanning and copies the staging area over the running
firmware, using a routine that runs entirely from RAM. On the RP2040 that
means `#[link_section = ".data"]` code with flash access through the ROM
functions (`flash_range_erase` / `flash_range_program`), interrupts off and
the second core parked. When the copy is done, it restarts.

**Power loss during the copy** (a few seconds). The old image is half
overwritten and the half won't start. To avoid that, a small permanent
second-stage loader at the start of flash checks a "copy in progress" marker
and finishes the copy from staging on the next start. This is the A/B and
marker scheme that `embassy-boot` already implements for the RP2040 and the
nRF52840. **Use `embassy-boot` rather than writing this by hand.** That moves
the firmware's start address, so the UF2 layout and the first USB install
change too: they become the loader plus the firmware.

### 4. App flow

- `updateFirmware` today updates each half over USB. It gains a path for a
  `CableTransport` session: begin, chunks with a progress bar, finish, apply.
  Then it waits for the far half to be heard over the cable again with the
  new version in `GetInfo`.
- Order: update the far half over the cable first, then the half on USB the
  usual way. If the half on USB went first and its new firmware couldn't talk
  to the old far half, the cable update would be stuck.
- `updateNeedsUsb` in `app/src/lib/firmware.ts` goes away for halves whose
  firmware supports the cable update.

### 5. Safety rules

- Refuse an image for another chip, or one that doesn't fit, in
  `BeginUpdate`.
- Never apply without a matching CRC.
- An image is only applied when the app asks for it after `FinishUpdate`
  succeeded. A half that restarts in the middle of a transfer keeps running
  its old firmware.
- With the loader from step 3, power loss at any point leaves a half that
  starts, either old or new.

## Testing

- `jmk-sim`: the transfer requests against `MemFlash`, including lost parts
  and repeated chunks, plus a simulated power cut between "marker set" and
  "copy done" that finishes on the next boot.
- App: a virtual split with one USB cable updated end to end, in a unit test
  like the cable test in `app/tests/unit/virtual.test.ts`.
- Hardware: the old V2b Svalboard as the test board. Pull the power during
  the copy on purpose.

## Steps

1. Move to `embassy-boot` on the RP2040. The first install over USB then
   carries the loader. No cable work yet.
2. Add the transfer requests and staging writes, tested in `jmk-sim`.
3. Bigger parts and windowed sending on the link.
4. App flow with progress, then try it on hardware.
5. nRF52840, once a board has run that port at all.

## Open questions

- Can the UF2 file the app carries be used as is? The far half would then
  strip the UF2 framing itself; otherwise the app sends the plain image.
- Should one image hold both halves' firmware? A split has the same chip on
  both halves today, but that isn't guaranteed.
- Does `embassy-boot`'s flash layout clash with where the settings live
  today? If it does, the first update with the loader has to move the
  settings without losing them.
