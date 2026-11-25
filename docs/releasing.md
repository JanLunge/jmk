# Releasing the desktop app

One command builds POG for macOS, Windows and Linux from the current commit
and publishes it as a GitHub release:

    scripts/release.sh

Installed apps look at the latest GitHub release, so publishing is also what
offers the update to everyone.

## Before running it

1. Count up `version` in `app/package.json`. The release is tagged `v<version>`.
2. If the firmware changed, count up `FIRMWARE_VERSION` in `crates/jmk-proto` too,
   or keyboards are not offered the firmware the new app carries.
3. Commit and merge to `main`. The script refuses uncommitted changes and
   commits that are not on `origin/main`.

`scripts/release.sh --build` builds everything into `app/release/v<version>/`
without tagging or uploading. `--skip-checks` leaves out `scripts/check.sh`.

## What happens

1. **Built once, on the machine you run it from:** both firmware files and the
   page (`dist`). They are packed into one archive with the Electron shell and
   the lockfile, so all three apps wrap the very same files.
2. **Three builds side by side:**
   - macOS (`mac-mini`): signed with the Developer ID certificate, notarised
     by Apple, for Apple silicon and Intel. `MAC_HOST=local` builds on the
     machine you run the script from instead.
   - Linux and Windows (`wdesk-linux`): both in one container,
     `electronuserland/builder:wine`, pinned by digest in the script. The
     Windows installer is cross-built there with Wine; it is not signed.
   - `scripts/check.sh`, at the same time. A failure stops the release.
3. **Checked:** every system's update feed (`latest*.yml`) is there, and macOS
   accepts the notarised app.
4. **Published:** uploaded to a draft release, which is made public only once
   every file is up. An installed app never sees half a release.

## How an installed app updates

`app/electron/updater.cjs` uses electron-updater. On start, every six hours,
and on "Check for updates" on the home screen it reads the feed of the latest
release. A newer version is downloaded in the background; the title bar then
offers "Restart to update". Closing the app installs it as well.

- macOS only updates a signed app. Windows updates through the installer.
- On Linux only the AppImage can replace itself.
- A build started with `npm start` does not look for updates.

## What makes it repeatable

- The release is one commit, named by the tag.
- `npm ci` and `cargo --locked` use the committed lockfiles.
- The Linux and Windows tools are one container image, pinned by digest.
- The firmware inside all three apps is the same file.

Two builds of the same commit are not byte for byte equal: signatures and
Apple's notarisation carry the time.

## The Mac mini

The Developer ID certificate is already there, in
`~/Library/Keychains/build.keychain-db`: Heaper's release workflow puts it
there and keeps it. `~/.pog-release.env` on the mini points the build at it:

    POG_KEYCHAIN=/Users/ulflunge/Library/Keychains/build.keychain-db
    POG_KEYCHAIN_PASSWORD=
    APPLE_KEYCHAIN_PROFILE=pog-notary
    APPLE_KEYCHAIN=/Users/ulflunge/Library/Keychains/build.keychain-db

The last two lines name the login for Apple's notarisation, stored once in
the same keychain with an app-specific password from account.apple.com:

    xcrun notarytool store-credentials pog-notary --apple-id <Apple ID> \
        --team-id BLH4PG2L7J --keychain ~/Library/Keychains/build.keychain-db

Without them the app is signed but not notarised, and the script refuses to
publish it.

To build on another Mac, give it Node, the certificate in a keychain, and the
same file. A Mac with the certificate in its login keychain needs no
`POG_KEYCHAIN` lines and builds with `MAC_HOST=local`.

## Setting up the Linux machine (once)

Docker, and the user in the `docker` group. The first build downloads the
container image (about 6 GB) and Electron; later builds reuse both from
`~/.cache/pog-build`.
