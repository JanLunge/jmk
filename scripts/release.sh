#!/usr/bin/env bash
# Builds the desktop app for macOS, Windows and Linux from the current commit
# and publishes it as a GitHub release, which is also where installed apps
# look for their update. See docs/releasing.md.
#
#   scripts/release.sh             build, check, publish v<version of app/package.json>
#   scripts/release.sh --build     build only; nothing is tagged or uploaded
#
# The parts that are the same everywhere (firmware, the page) are built once,
# here; the build machines only wrap them into an app.
set -euo pipefail
cd "$(dirname "$0")/.."

# Where each app is built. 'local' builds the Mac app on this machine.
MAC_HOST=${MAC_HOST:-mac-mini}
LINUX_HOST=${LINUX_HOST:-wdesk-linux}
# Linux and Windows are both built in this container; the digest pins it.
IMAGE=${IMAGE:-electronuserland/builder:wine@sha256:f3a8d451994c881d0972e7b1846ee4d4fdc15eb41fc6b1f350b38cbf99ca90fe}
RUST=${RUST:-$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin}

publish=1
checks=1
for arg in "$@"; do
  case $arg in
    --build) publish=0 ;;
    --skip-checks) checks=0 ;;
    *) echo "unknown option $arg" >&2; exit 2 ;;
  esac
done

export PATH="$RUST:$PATH"
version=$(node -p "require('./app/package.json').version")
tag=v$version
commit=$(git rev-parse HEAD)
out=$PWD/app/release/$tag
started=$SECONDS
say() { printf '\n== %s (%dm%02ds)\n' "$1" $(((SECONDS - started) / 60)) $(((SECONDS - started) % 60)); }

if ((publish)); then
  [[ -z $(git status --porcelain) ]] || { echo "Uncommitted changes: a release is built from a commit." >&2; exit 1; }
  git fetch -q origin
  git merge-base --is-ancestor "$commit" origin/main || { echo "This commit is not on origin/main." >&2; exit 1; }
  ! gh release view "$tag" >/dev/null 2>&1 || { echo "$tag is released already. Count up the version in app/package.json." >&2; exit 1; }
fi

rm -rf "$out"
mkdir -p "$out/logs"

say "Firmware and page, built once"
(cd firmware/rp2040 && cargo build --release --locked)
cargo run -q --locked -p jmk-uf2 -- rp2040 firmware/rp2040/target/thumbv6m-none-eabi/release/jmk-rp2040 target/jmk-rp2040.uf2
(cd firmware/rp2350 && cargo build --release --locked)
cargo run -q --locked -p jmk-uf2 -- rp2350 firmware/rp2350/target/thumbv8m.main-none-eabihf/release/jmk-rp2350 target/jmk-rp2350.uf2
(cd firmware/nrf52840 && cargo build --release --locked)
cargo run -q --locked -p jmk-uf2 -- nrf52840 firmware/nrf52840/target/thumbv7em-none-eabihf/release/jmk-nrf52840 target/jmk-nrf52840.uf2
(cd app && npm ci --no-audit --no-fund --silent && rm -rf firmware && npm run firmware --silent && npm run build --silent)
# Only what goes into the app, so every machine wraps the same files.
COPYFILE_DISABLE=1 tar -czf "$out/stage.tgz" -C app package.json package-lock.json electron dist firmware build

# --- The three builds, side by side -------------------------------------------
# What runs on a Mac, here or over SSH. Signing and notarising read their
# secrets from ~/.pog-release.env on that Mac; see docs/releasing.md.
mac_steps='
set -euo pipefail
cd "$1" && tar -xzf stage.tgz && rm stage.tgz
[[ -f ~/.pog-release.env ]] && { set -a; source ~/.pog-release.env; set +a; }
if [[ -n ${POG_KEYCHAIN:-} ]]; then
  security unlock-keychain -p "$POG_KEYCHAIN_PASSWORD" "$POG_KEYCHAIN"
  export CSC_KEYCHAIN=$POG_KEYCHAIN
fi
npm ci --no-audit --no-fund --silent
npx electron-builder --mac --publish never
if [[ -n ${APPLE_API_KEY:-}${APPLE_ID:-}${APPLE_KEYCHAIN_PROFILE:-} ]]; then
  # Apple notarises the app itself; the disk image and the zip only carry it.
  for built in release/mac*/POG.app; do
    xcrun stapler validate -q "$built"
    spctl --assess --type execute "$built"
  done
  touch release/NOTARISED
else
  echo "No notarising secrets on this Mac: the app is signed but macOS will warn about it."
fi
'
build_mac() {
  if [[ $MAC_HOST == local ]]; then
    local dir
    dir=$(mktemp -d)
    cp "$out/stage.tgz" "$dir/"
    bash -c "$mac_steps" mac "$dir"
    cp "$dir"/release/*.{dmg,zip,blockmap,yml} "$out/"
    [[ -f $dir/release/NOTARISED ]] && touch "$out/NOTARISED"
    rm -rf "$dir"
  else
    local dir=pog-build/$tag
    ssh "$MAC_HOST" "rm -rf $dir && mkdir -p $dir"
    scp -q "$out/stage.tgz" "$MAC_HOST:$dir/"
    ssh "$MAC_HOST" "zsh -lc 'bash -s $dir'" <<<"$mac_steps"
    ssh "$MAC_HOST" "cd $dir/release && tar -cf - *.dmg *.zip *.blockmap *.yml \$(ls NOTARISED 2>/dev/null)" | tar -xf - -C "$out"
    ssh "$MAC_HOST" "rm -rf $dir"
  fi
}

build_linux_windows() {
  local dir=pog-build/$tag
  ssh "$LINUX_HOST" "rm -rf $dir && mkdir -p $dir ~/.cache/pog-build/electron ~/.cache/pog-build/builder ~/.cache/pog-build/npm"
  scp -q "$out/stage.tgz" "$LINUX_HOST:$dir/"
  ssh "$LINUX_HOST" "cd $dir && tar -xzf stage.tgz && rm stage.tgz && docker run --rm \
    -v \$PWD:/project -w /project \
    -v ~/.cache/pog-build/electron:/root/.cache/electron \
    -v ~/.cache/pog-build/builder:/root/.cache/electron-builder \
    -v ~/.cache/pog-build/npm:/root/.npm \
    $IMAGE bash -c 'npm ci --no-audit --no-fund --silent && npx electron-builder --linux --win --publish never; code=\$?; chown -R '\$(id -u):\$(id -g)' /project; exit \$code'"
  ssh "$LINUX_HOST" "cd $dir/release && tar -cf - *.exe *.AppImage *.blockmap *.yml" | tar -xf - -C "$out"
  ssh "$LINUX_HOST" "rm -rf $dir"
}

say "Building: macOS on $MAC_HOST, Linux and Windows on $LINUX_HOST"
build_mac >"$out/logs/mac.log" 2>&1 &
mac=$!
build_linux_windows >"$out/logs/linux-windows.log" 2>&1 &
linwin=$!
check=
if ((checks)); then
  scripts/check.sh >"$out/logs/check.log" 2>&1 &
  check=$!
fi

failed=0
wait $mac || { failed=1; echo "The macOS build failed:"; tail -20 "$out/logs/mac.log"; }
say "macOS done"
wait $linwin || { failed=1; echo "The Linux and Windows build failed:"; tail -20 "$out/logs/linux-windows.log"; }
say "Linux and Windows done"
if [[ -n $check ]]; then
  wait $check || { failed=1; echo "The checks failed:"; tail -30 "$out/logs/check.log"; }
  say "Checks done"
fi
((failed)) && exit 1

rm "$out/stage.tgz" "$out"/builder-*.yml 2>/dev/null || true
# Each system's update feed must be there, or its installed apps never hear of this release.
for feed in latest-mac.yml latest.yml latest-linux.yml; do
  [[ -f $out/$feed ]] || { echo "Missing $feed" >&2; exit 1; }
done
if [[ ! -f $out/NOTARISED ]]; then
  echo "The Mac app is not notarised: macOS would refuse it." >&2
  ((publish)) && exit 1
fi
rm -f "$out/NOTARISED"
ls -lh "$out" | grep -v logs

if ((!publish)); then
  say "Built, not published: $out"
  exit 0
fi

say "Publishing $tag"
# A draft first: an installed app must never see a release with files missing.
gh release create "$tag" --draft --target "$commit" --title "POG $version" --generate-notes \
  "$out"/*.dmg "$out"/*.zip "$out"/*.exe "$out"/*.AppImage "$out"/*.blockmap "$out"/*.yml
gh release edit "$tag" --draft=false
git fetch -q origin tag "$tag"
say "Released: $(gh release view "$tag" --json url -q .url)"
