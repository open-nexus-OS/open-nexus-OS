#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: The FIT a board's SPL loads from the `uboot` partition (ADR-0066,
#          TASK-0260B): nxboot — from the last OS build, the same loader every
#          QEMU lane boots — and the board's device tree, assembled by `mkimage`
#          from config/board/<board>/nexus.its. The payload is nxboot's flat image
#          padded to its whole memory footprint (text through .bss and its stack,
#          from the ELF's own symbols): the SPL places the tree right after the
#          payload's bytes, then grows it in place, and so does OpenSBI — an
#          unpadded payload would have them inside nxboot's .bss, zeroed by its
#          first instructions. Reproducible: no build time is recorded
#          (SOURCE_DATE_EPOCH=0), two builds give the same bytes.
# OWNERS:  @tools-team @reliability
# STATUS:  Functional
# API_STABILITY: Stable (arguments)
# TEST_COVERAGE: TASK-0260B (two builds byte-identical; the FIT's shape checked
#                with dumpimage before it is used); the board boot itself
# DEPENDS_ON: mkimage + dumpimage (u-boot tools), dtc + cpp (scripts/build-board-dtb.sh),
#             llvm-nm + llvm-objcopy (rustup llvm-tools), the OS build's nxboot ELF
#
# Usage:
#   scripts/build-fit.sh [BOARD] [OUT]   # default: bpi-f3 → build/board/<board>/nexus.itb
#
# Exit codes: 0 built · 1 a tool or an input missing · 2 a build step failed or the FIT is not
#             what the SPL expects

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BOARD="${1:-bpi-f3}"
# Absolute: mkimage runs inside the staging directory.
OUT="$(realpath -m "${2:-$ROOT/build/board/$BOARD/nexus.itb}")"
TARGET_ROOT="${CARGO_TARGET_DIR:-$ROOT/target}"
ELF="$TARGET_ROOT/riscv64imac-unknown-none-elf/release/nxboot"
ITS="$ROOT/config/board/$BOARD/nexus.its"
# The payload's slot in the vendor FIT (measured) and the partition the FIT goes into.
LOAD=0x00200000
SLOT_BYTES=$((2 * 1024 * 1024))

log() { printf '\033[1;34m[build-fit]\033[0m %s\n' "$*"; }
err() { printf '\033[1;31m[build-fit][error]\033[0m %s\n' "$*" >&2; }

for t in mkimage dumpimage dtc cpp; do
  command -v "$t" >/dev/null 2>&1 || { err "$t missing — scripts/install-deps.sh"; exit 1; }
done
tools=""
for c in "$HOME"/.rustup/toolchains/*/lib/rustlib/*/bin; do
  [ -x "$c/llvm-nm" ] && [ -x "$c/llvm-objcopy" ] && { tools=$c; break; }
done
[ -n "$tools" ] || { err "llvm-nm/llvm-objcopy missing — rustup component add llvm-tools-preview"; exit 1; }
[ -f "$ITS" ] || { err "no FIT source at $ITS"; exit 1; }
[ -f "$ELF" ] || { err "no nxboot at $ELF — run: just build-os-workspace"; exit 1; }

stage="$(dirname "$OUT")/fit"
rm -rf "$stage"
mkdir -p "$stage"

# nxboot's footprint from its own link: the image starts at __image_start (0, a static PIE), its
# loadable bytes end at __load_end, .bss and the loader stack at __image_end.
sym() { "$tools/llvm-nm" "$ELF" | awk -v s="$1" '$3 == s { print $1 }'; }
start="$(sym __image_start)"; load_end="$(sym __load_end)"; end="$(sym __image_end)"
[ -n "$start" ] && [ -n "$load_end" ] && [ -n "$end" ] || { err "nxboot's link symbols are missing"; exit 2; }
start=$((16#$start)); load_end=$((16#$load_end)); end=$((16#$end))
"$tools/llvm-objcopy" -O binary "$ELF" "$stage/nxboot.img"
flat="$(stat -c %s "$stage/nxboot.img")"
[ "$start" = 0 ] && [ "$flat" = "$((load_end - start))" ] || {
  err "nxboot's flat image is $flat bytes, its loadable span $((load_end - start)) (start $start)"; exit 2; }
truncate -s "$((end - start))" "$stage/nxboot.img"
log "nxboot: $flat bytes loaded, padded to its footprint $((end - start)) (.bss + stack)"

"$ROOT/scripts/build-board-dtb.sh" "$BOARD" "$stage/board.dtb" >/dev/null
cp "$ITS" "$stage/nexus.its"
(cd "$stage" && SOURCE_DATE_EPOCH=0 mkimage -f nexus.its "$OUT" >/dev/null) || { err "mkimage failed"; exit 2; }

# The shape the SPL and OpenSBI take (measured on the vendor FIT): the payload at the slot, the
# default configuration naming it and the tree, and the whole FIT inside its partition.
listing="$(dumpimage -l "$OUT")"
grep -q "Load Address: $LOAD" <<<"$listing" && grep -q "Entry Point:  $LOAD" <<<"$listing" \
  && grep -q "Default Configuration: 'conf-1'" <<<"$listing" && grep -q "Loadables:    nxboot" <<<"$listing" \
  || { err "the FIT does not have the shape the SPL expects:"; echo "$listing" >&2; exit 2; }
# The payload the SPL loads covers nxboot's whole footprint — the tree lands past its stack.
payload="$(awk '/^ Image [0-9]+ \(nxboot\)/ { in_img = 1 } in_img && /Data Size:/ { print $3; exit }' <<<"$listing")"
[ "$payload" = "$((end - start))" ] || {
  err "the FIT's nxboot payload is ${payload:-?} bytes, nxboot's footprint $((end - start))"; exit 2; }
size="$(stat -c %s "$OUT")"
[ "$size" -le "$SLOT_BYTES" ] || { err "the FIT is $size bytes, its partition $SLOT_BYTES"; exit 2; }
log "$OUT ($size bytes, sha256 $(sha256sum "$OUT" | cut -c1-16)…)"
