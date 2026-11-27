#!/usr/bin/env bash
# Builds the firmware from this checkout and flashes it to every connected
# JMK keyboard, one after the other, such as both halves of a split. Each
# keyboard gets the image for the chip it reports. Run from anywhere.
#
#   scripts/flashall.sh        ask before flashing
#   scripts/flashall.sh -y     do not ask
#
# The halves of a split cannot talk to each other between the two flashes.
# An RP2350 gets the image for the package with 30 GPIOs.
set -euo pipefail
cd "$(dirname "$0")/.."

ask=1
for arg in "$@"; do
  case $arg in
    -y) ask=0 ;;
    *) echo "usage: scripts/flashall.sh [-y]" >&2; exit 2 ;;
  esac
done

cargo build -q -p jmk-cli
jmk=target/debug/jmk

# One line per keyboard: "<chip id>  <chip>, firmware <version>", or the
# hidraw device and an error for one that does not answer.
ids=()
mcus=()
while read -r line; do
  if [[ $line =~ ^([0-9a-f]+)\ +([A-Za-z0-9]+), ]]; then
    ids+=("${BASH_REMATCH[1]}")
    mcus+=("${BASH_REMATCH[2],,}")
  else
    echo "a keyboard did not answer: $line" >&2
    exit 1
  fi
done < <("$jmk" devices)
((${#ids[@]})) || { echo "no JMK keyboard found" >&2; exit 1; }

# Build each chip's image once. Cargo names the file it built, so no
# chip's target directory is spelled out here.
declare -A images=()
for mcu in $(printf '%s\n' "${mcus[@]}" | sort -u); do
  [[ -d firmware/$mcu ]] || { echo "no firmware for $mcu in firmware/" >&2; exit 1; }
  echo "building firmware for $mcu"
  elf=$(cd "firmware/$mcu" && cargo build --release --message-format=json-render-diagnostics \
    | sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' | tail -n 1)
  [[ -n $elf ]] || { echo "the $mcu build named no executable" >&2; exit 1; }
  cargo run -q -p jmk-uf2 -- "$mcu" "$elf" "target/jmk-$mcu.uf2"
  images[$mcu]=target/jmk-$mcu.uf2
done

echo
for i in "${!ids[@]}"; do
  echo "  ${ids[i]}  ${mcus[i]}  <- ${images[${mcus[i]}]}"
done
if ((ask)); then
  read -rp "Flash ${#ids[@]} keyboard(s)? Each restarts and stops typing for a few seconds. [y/N] " answer
  [[ $answer == [yY]* ]] || { echo "nothing flashed"; exit 1; }
fi

for i in "${!ids[@]}"; do
  echo "flashing ${ids[i]}"
  if ! "$jmk" --device "${ids[i]}" flash "${images[${mcus[i]}]}"; then
    echo "flashing ${ids[i]} failed; keyboards flashed before it run the new firmware" >&2
    exit 1
  fi
done
echo "all ${#ids[@]} keyboard(s) flashed"
