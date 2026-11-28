# POG

The configurator for keyboards running JMK firmware, and the second version
of POG. It sets up a keyboard: controller and protected pins, key
detection, layout, keymap, hold-tap, combos, macros, lights, split halves,
firmware updates and keyboard files.

It is a Vue application that runs in two places:

- the **desktop app** (Electron), which also installs firmware onto a board
  in bootloader mode, and
- any Chromium browser, where it reaches keyboards through WebHID.

Everything that touches the protocol's binary format is Rust compiled to
WebAssembly (`crates/jmk-wasm`). The same module runs **virtual keyboards**:
the real device logic on simulated wiring. They are offered on the home
screen for trying the app, and the tests use them.

## Commands

```sh
npm install
npm run dev        # in a browser, with hot reload (run `npm run wasm` first)
npm run start      # build and open the desktop app
npm run start:fw   # the same, after building the firmware it carries anew
npm run firmware   # copy the built firmware images into the desktop app
npm test           # unit tests (logic, and the session layer on virtual keyboards)
npm run e2e        # the whole app in headless Chromium on virtual keyboards
JMK_DESKTOP=1 xvfb-run -a npx playwright test desktop   # the desktop shell
```

Add `JMK_REAL=1` to the last command with a JMK keyboard attached to also
check that the desktop app reads it. On Linux the user needs access to the
keyboard's hidraw device; the udev rule in `linux/` does that (see "Linux" in
the main README). The desktop app installs it on request, through `pkexec`.

The e2e tests start their own preview server on port 4173 and refuse to run
against one that is already there, since that may be another checkout's
build. `E2E_PORT=4273 npm run e2e` uses another port.

The desktop app started with `npm run start` can be inspected while it runs:
it listens for DevTools on `http://127.0.0.1:9333` (`POG_CDP_PORT` picks
another port, `POG_CDP_PORT=0` none). Released apps never open that port.

## Layout

| Path | What it is |
| --- | --- |
| `src/lib/protocol.ts` | The wire types as JSON; mirrors `crates/jmk-proto`. |
| `src/lib/transport.ts`, `client.ts`, `session.ts` | USB and virtual transports, requests, and the state of one connected keyboard. |
| `src/lib/infer.ts`, `collector.ts` | Working out the wiring from what the probe saw. |
| `src/lib/boards.ts` | Controller boards the app can draw. |
| `src/views` | Home, setup wizard, and the editor tabs. |
| `electron/` | The desktop shell. |
