#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Flash the reference board over its boot-ROM download mode with plain
#          `fastboot` (TASK-0327). The protocol, as the vendor's own recipe
#          (fastboot.yaml in the release archive) and three independent
#          write-ups agree:
#            1. boot ROM (361c:1001): `fastboot stage FSBL.bin; fastboot continue`
#               — the SPL runs from SRAM, trains DDR, and waits again;
#            2. `fastboot stage u-boot.itb; fastboot continue` — the vendor
#               U-Boot runs from RAM and offers the flashing mode;
#            3. `fastboot flash <partition> <file>` per partition, the GPT
#               first (`fastboot flash gpt partition_universal.json`).
#          Steps 1–2 write NOTHING; that is what --stage-only does, and it is
#          how the board's variables are read (`fastboot getvar all`) without
#          touching it. Step 3 is destructive for the eMMC; the stock system
#          on the microSD card is not touched (the boot ROM tries the card
#          first), and the script asks before writing.
#
#          What gets written today: the vendor BOOT VEHICLE only — the GPT
#          from the vendor partition table and the bootloader partitions
#          (bootinfo, fsbl, env, opensbi, uboot), never its OS volumes. Our own
#          partitions (`userspace/storage/src/layout.rs`) join this recipe with
#          Block 1's boot chain (TASK-0260/0260B), when the image `nx image`
#          builds carries the boot-ROM head itself; the banner says which
#          chain was flashed so this can never read as "our chain boots".
# OWNERS:  @tools-team
# STATUS:  Functional
# API_STABILITY: Stable (flags); the partition set changes with TASK-0260B
# TEST_COVERAGE: TASK-0327 T2 against the desk board (--stage-only measured first)
# DEPENDS_ON: fastboot (android-tools), scripts/board-devices.sh, scripts/fetch-board-inputs.sh
#
# Usage:
#   scripts/board-flash.sh --stage-only     # boot ROM → SPL → U-Boot in RAM, print getvars, write nothing
#   scripts/board-flash.sh                  # ... then flash the vendor boot vehicle to eMMC (asks first)
#   scripts/board-flash.sh --yes            # no confirmation prompt
#   scripts/board-flash.sh --skip-stage     # board already sits in U-Boot fastboot mode
#
# Exit codes: 0 done · 1 tool/inputs missing · 2 aborted or fastboot failed · 3 board not in download mode

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BOARD="bpi-f3"
VENDOR="$ROOT/resources/board/$BOARD/vendor"

STAGE_ONLY=0; ASSUME_YES=0; SKIP_STAGE=0
for arg in "$@"; do
  case "$arg" in
    --stage-only) STAGE_ONLY=1 ;;
    --yes|-y)     ASSUME_YES=1 ;;
    --skip-stage) SKIP_STAGE=1 ;;
    -h|--help)    sed -n '34,40p' "$0"; exit 0 ;;
    *) echo "[error] unknown flag: $arg" >&2; exit 1 ;;
  esac
done

log()  { printf '\033[1;34m[board-flash]\033[0m %s\n' "$*"; }
err()  { printf '\033[1;31m[board-flash][error]\033[0m %s\n' "$*" >&2; }

command -v fastboot >/dev/null 2>&1 || { err "fastboot missing — scripts/install-deps.sh (android-tools)"; exit 1; }
"$ROOT/scripts/fetch-board-inputs.sh" --check >/dev/null 2>&1 || {
  err "vendor boot pieces not fetched — run: just board-inputs"; exit 1; }

mode="$("$ROOT/scripts/board-devices.sh" --mode || true)"
if [ "$mode" != "download" ]; then
  err "board is not in download/fastboot mode (seen: ${mode:-none})."
  err "Hold the download key (FDL) while resetting the board, or type 'fastboot usb 0' at its U-Boot prompt; then re-run."
  exit 3
fi

# `fastboot` talks to the first device; a second one would make this ambiguous.
n="$(fastboot devices 2>/dev/null | grep -c . || true)"
[ "$n" -eq 1 ] || { err "expected exactly one fastboot device, found $n"; exit 3; }

fb() { # log + run one fastboot command; any failure is fatal
  log "fastboot $*"
  if ! fastboot "$@"; then err "fastboot $* failed"; exit 2; fi
}

if [ "$SKIP_STAGE" != 1 ]; then
  # A boot ROM answers `version-brom`; a U-Boot already in fastboot mode does
  # not — the vendor recipe uses exactly this to skip the staging steps.
  if fastboot getvar version-brom >/dev/null 2>&1; then
    log "boot ROM answered (version-brom) — staging the SPL, then U-Boot, into RAM"
    fb stage "$VENDOR/factory/FSBL.bin"
    fb continue
    sleep 1                      # the SPL trains DDR and re-enumerates
    fb stage "$VENDOR/u-boot.itb"
    fb continue
    sleep 2                      # U-Boot re-enumerates in fastboot mode
  else
    log "no boot-ROM answer — assuming the board already runs U-Boot's fastboot mode"
  fi
fi

log "board variables (U-Boot fastboot):"
fastboot getvar all 2>&1 | sed 's/^/    /' || true

if [ "$STAGE_ONLY" = 1 ]; then
  log "--stage-only: nothing written. The board sits in U-Boot fastboot mode until reset."
  exit 0
fi

# --- write the vendor boot vehicle -------------------------------------------
PARTS=(
  "bootinfo  factory/bootinfo_sd.bin"    # the vendor's universal table uses the SD header for eMMC too (measured: partition_universal.json)
  "fsbl      factory/FSBL.bin"
  "env       env.bin"
  "opensbi   fw_dynamic.itb"
  "uboot     u-boot.itb"
)
echo
log "ABOUT TO WRITE THE eMMC: GPT from partition_universal.json + ${#PARTS[@]} bootloader partitions"
log "chain flashed = VENDOR boot vehicle (SPL → OpenSBI → vendor U-Boot); our OS volumes come with TASK-0260B"
log "the stock system on the microSD card is not touched"
if [ "$ASSUME_YES" != 1 ]; then
  printf '[board-flash] Continue? [y/N] '
  read -r reply
  case "$reply" in y|Y|yes|YES) ;; *) err "aborted by user (board stays in fastboot mode)."; exit 2 ;; esac
fi

fb flash gpt "$VENDOR/partition_universal.json"
for entry in "${PARTS[@]}"; do
  read -r part file _ <<<"$entry"
  fb flash "$part" "$VENDOR/$file"
done
log "written. Resetting the board (it boots the microSD card first; eMMC carries the boot vehicle only)."
fb reboot
