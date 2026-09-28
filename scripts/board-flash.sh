#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Flash the reference board over its boot-ROM download mode with plain
#          `fastboot` (TASK-0327, TASK-0260 P2). The vehicle, as the vendor's
#          own recipe (fastboot.yaml in the release archive) stages it:
#            1. boot ROM (361c:1001): `fastboot stage FSBL.bin; fastboot continue`
#               — the SPL runs from SRAM, trains DDR, and waits again;
#            2. `fastboot stage u-boot.itb; fastboot continue` — the vendor
#               U-Boot runs from RAM and offers the flashing mode.
#          Steps 1–2 write NOTHING; that is what --stage-only does.
#
#          What gets written is OUR disk, byte for byte, from a flash plan
#          (`nx image flash-plan`, docs/board/measurements/2026-09-26-boot-medium):
#          the eMMC's user area (our GPT with its protective MBR, the head the
#          SPL loads by name, our volumes, the backup GPT at the disk's end)
#          and its boot0 hardware partition (the boot-ROM header and the SPL).
#          The vehicle's own `flash gpt <json>` builds a GPT of its own (every
#          partition basic data) and its JSON regions compute offsets in 32
#          bits, so neither is used: each region is declared as a raw
#          partition (`oem env:set fastboot_raw_partition_<name>:<start>
#          <sectors> [mmcpart <n>]` — 64-bit block addresses, measured), its
#          size read back (`getvar partition-size:<name>`) before anything is
#          written, then flashed. --verify reads every region back from the
#          stock system (it boots from the microSD first: DIP 1+2 default)
#          over adb and compares the plan's digests.
# OWNERS:  @tools-team
# STATUS:  Functional
# API_STABILITY: Stable (flags)
# TEST_COVERAGE: TASK-0327 T2 (--stage-only) and TASK-0260 P2 (--plan, --verify) against
#                the desk board; the plan itself: tools/nx/tests/image_flash_cli.rs
# DEPENDS_ON: fastboot (android-tools), adb (--verify), python3, scripts/board-devices.sh,
#             scripts/fetch-board-inputs.sh
#
# Usage:
#   scripts/board-flash.sh --stage-only          # boot ROM → SPL → U-Boot in RAM, print the board's variables, write nothing
#   scripts/board-flash.sh --plan DIR            # ... then write the flash plan in DIR to the eMMC (asks first)
#   scripts/board-flash.sh --plan DIR --yes      # no confirmation prompt
#   scripts/board-flash.sh --verify DIR          # read every region of the plan back from the stock system (adb)
#   --skip-stage                                 # the board already sits in U-Boot fastboot mode
#
# Exit codes: 0 done · 1 tool/inputs missing · 2 aborted, fastboot failed or a region mismatched ·
#             3 board not in the mode the step needs

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BOARD="bpi-f3"
VENDOR="$ROOT/resources/board/$BOARD/vendor"
# The eMMC as the stock system names it (mmc2; measured 2026-09-22) and its boot0.
STOCK_EMMC="/dev/mmcblk2"

STAGE_ONLY=0; ASSUME_YES=0; SKIP_STAGE=0; PLAN=""; VERIFY=""
while [ $# -gt 0 ]; do
  case "$1" in
    --stage-only) STAGE_ONLY=1 ;;
    --yes|-y)     ASSUME_YES=1 ;;
    --skip-stage) SKIP_STAGE=1 ;;
    --plan)       PLAN="${2:?--plan needs a directory}"; shift ;;
    --verify)     VERIFY="${2:?--verify needs a directory}"; shift ;;
    -h|--help)    sed -n '35,42p' "$0"; exit 0 ;;
    *) echo "[error] unknown flag: $1" >&2; exit 1 ;;
  esac
  shift
done

log()  { printf '\033[1;34m[board-flash]\033[0m %s\n' "$*"; }
err()  { printf '\033[1;31m[board-flash][error]\033[0m %s\n' "$*" >&2; }

# The plan's regions, one per line: name hwpart start_lba sectors file sha256.
regions() {
  python3 - "$1/plan.json" <<'PY'
import json, sys
plan = json.load(open(sys.argv[1]))
for r in plan["regions"]:
    print(r["name"], r["hwpart"], r["start_lba"], r["sectors"], r["file"], r["sha256"])
PY
}

plan_field() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$1/plan.json" "$2"; }

# --- verify: read the plan back from the stock system ----------------------------------
if [ -n "$VERIFY" ]; then
  [ -f "$VERIFY/plan.json" ] || { err "no plan.json in $VERIFY"; exit 1; }
  command -v adb >/dev/null 2>&1 || { err "adb missing — scripts/install-deps.sh (android-tools)"; exit 1; }
  [ "$(timeout 10 adb get-state 2>/dev/null)" = "device" ] || {
    err "no stock system on adb — let the board boot the microSD (reset), then re-run"; exit 3; }
  sectors="$(timeout 10 adb shell cat /sys/block/mmcblk2/size | tr -d '\r')"
  want="$(plan_field "$VERIFY" disk_sectors)"
  [ "$sectors" = "$want" ] || { err "the eMMC has $sectors sectors, the plan was built for $want"; exit 2; }
  # RFC-0107: the trace partition is written by every boot, so it is read as zero — the plan's
  # regions carry it zero (an older plan without the field: nothing is skipped).
  trace_start="$(python3 -c 'import json,sys; t=json.load(open(sys.argv[1])).get("trace"); print(t["start_lba"] if t else "")' "$VERIFY/plan.json")"
  trace_count="$(python3 -c 'import json,sys; t=json.load(open(sys.argv[1])).get("trace"); print(t["sectors"] if t else "")' "$VERIFY/plan.json")"
  bad=0
  while read -r name hwpart start count _file sha; do
    dev="$STOCK_EMMC"; [ "$hwpart" = 1 ] && dev="${STOCK_EMMC}boot0"
    # The region's bytes, with the trace partition's span (if it lies in this region of the
    # user area) replaced by zeros — three reads piped into one digest.
    rd="dd if=$dev bs=1M iflag=skip_bytes,count_bytes"
    cmd="$rd skip=$((start * 512)) count=$((count * 512))"
    if [ "$hwpart" = 0 ] && [ -n "$trace_start" ]; then
      o0=$(( start > trace_start ? start : trace_start )); o1=$(( start + count < trace_start + trace_count ? start + count : trace_start + trace_count ))
      if [ "$o1" -gt "$o0" ]; then
        cmd="{ $rd skip=$((start * 512)) count=$(((o0 - start) * 512)); dd if=/dev/zero bs=512 count=$((o1 - o0)); $rd skip=$((o1 * 512)) count=$(((start + count - o1) * 512)); }"
      fi
    fi
    # stdin from /dev/null: `adb shell` reads it, and would swallow the rest of the regions.
    got="$(timeout 300 adb shell "$cmd 2>/dev/null | sha256sum" </dev/null | cut -c1-64)"
    if [ "$got" = "$sha" ]; then
      log "read back $name ($dev @ $start, $count sectors): sha256 matches"
    else
      err "read back $name ($dev @ $start, $count sectors): sha256 $got, the plan says $sha"; bad=1
    fi
  done < <(regions "$VERIFY")
  [ "$bad" = 0 ] || exit 2
  log "every region reads back as planned"
  exit 0
fi

# --- stage the vehicle (writes nothing) ------------------------------------------------
command -v fastboot >/dev/null 2>&1 || { err "fastboot missing — scripts/install-deps.sh (android-tools)"; exit 1; }
"$ROOT/scripts/fetch-board-inputs.sh" --check >/dev/null 2>&1 || {
  err "vendor boot pieces not fetched — run: just board-inputs"; exit 1; }
if [ -n "$PLAN" ]; then
  [ -f "$PLAN/plan.json" ] || { err "no plan.json in $PLAN — nx image flash-plan"; exit 1; }
fi

mode="$("$ROOT/scripts/board-devices.sh" --mode || true)"
case "$mode" in
  download) ;;                       # boot ROM: staging needed
  fastboot) SKIP_STAGE=1 ;;          # U-Boot already in RAM: staging would boot the board away
  *) err "board is not in download/fastboot mode (seen: ${mode:-none})."
     err "Hold the download key (FDL) while resetting the board, or type 'fastboot usb 0' at its U-Boot prompt; then re-run."
     exit 3 ;;
esac

# `fastboot` talks to the first device; a second one would make this ambiguous.
n="$(fastboot devices 2>/dev/null | grep -c . || true)"
[ "$n" -eq 1 ] || { err "expected exactly one fastboot device, found $n"; exit 3; }

fb() { # log + run one fastboot command; any failure is fatal (stdin kept off the region loop)
  log "fastboot $*"
  if ! fastboot "$@" </dev/null; then err "fastboot $* failed"; exit 2; fi
}

if [ "$SKIP_STAGE" != 1 ]; then
  # The USB product string decided this (board-devices --mode), not
  # `getvar version-brom`: the vendor U-Boot answers that variable too, and a
  # `stage` + `continue` against it made the board boot away mid-recipe
  # (measured 2026-09-21). `version-brom` is printed for the record only.
  brom="$(timeout 3 fastboot getvar version-brom 2>&1 | sed -n 's/^version-brom: //p' | head -1)"
  log "boot ROM (version-brom ${brom:-?}) — staging the SPL, then U-Boot, into RAM"
  fb stage "$VENDOR/factory/FSBL.bin"
  fb continue
  sleep 1                      # the SPL trains DDR and re-enumerates
  fb stage "$VENDOR/u-boot.itb"
  fb continue
  sleep 2                      # U-Boot re-enumerates in fastboot mode
  if [ "$("$ROOT/scripts/board-devices.sh" --mode || true)" != "fastboot" ]; then
    err "U-Boot did not come up in fastboot mode after staging (board-devices sees: $("$ROOT/scripts/board-devices.sh" --mode || echo none))"
    exit 2
  fi
else
  log "U-Boot fastboot mode already up — staging skipped"
fi

# Neither the boot ROM nor the vendor U-Boot implements `getvar all` (measured
# 2026-09-21: "Variable not implemented"); these are the variables that answer.
log "board variables (U-Boot fastboot):"
product=""; max_download=""
for v in product version-bootloader serialno blk-size mtd-size max-download-size current-slot; do
  val="$(timeout 5 fastboot getvar "$v" 2>&1 | head -1 | sed -n "s/^$v: //p")"
  printf '    %-18s %s\n' "$v" "${val:-<not implemented>}"
  case "$v" in product) product="$val" ;; max-download-size) max_download="$val" ;; esac
done

if [ "$STAGE_ONLY" = 1 ] || [ -z "$PLAN" ]; then
  log "nothing written. The board sits in U-Boot fastboot mode until reset."
  exit 0
fi

# --- write the plan ----------------------------------------------------------------------
chunk="$(plan_field "$PLAN" chunk_bytes)"
if [ "$product" != "k1-x" ] || [ -z "$max_download" ] || [ $((max_download)) -lt "$chunk" ]; then
  err "refusing to flash: product='${product:-?}' max-download-size='${max_download:-?}' (the plan's chunks are $chunk bytes)"
  exit 2
fi
echo
log "ABOUT TO WRITE THE eMMC from $PLAN:"
while read -r name hwpart start count file _sha; do
  printf '    %-9s hwpart %s  lba %-9s %9s sectors  %s\n' "$name" "$hwpart" "$start" "$count" "$file"
done < <(regions "$PLAN")
log "the stock system on the microSD card is not touched"
if [ "$ASSUME_YES" != 1 ]; then
  printf '[board-flash] Continue? [y/N] '
  read -r reply
  case "$reply" in y|Y|yes|YES) ;; *) err "aborted by user (board stays in fastboot mode)."; exit 2 ;; esac
fi

while read -r name hwpart start count file _sha; do
  desc="$start $count"
  [ "$hwpart" = 0 ] || desc="$desc mmcpart $hwpart"
  fb oem "env:set" "fastboot_raw_partition_${name}:${desc}"
  size="$(timeout 5 fastboot getvar "partition-size:$name" 2>&1 </dev/null | sed -n "s/^partition-size:$name: //p")"
  if [ -z "$size" ] || [ $((size)) -ne $((count * 512)) ]; then
    err "the vehicle does not see $name as $count sectors (partition-size: ${size:-none}) — nothing written for it"
    exit 2
  fi
  fb flash "$name" "$PLAN/$file"
done < <(regions "$PLAN")
log "written. Resetting the board (it boots the microSD first); then: scripts/board-flash.sh --verify $PLAN"
fb reboot
