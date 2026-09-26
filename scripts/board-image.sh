#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: The board's disk and its flash plan (TASK-0260 P2). `nx image build --target
#          <board>` builds the eMMC's user area byte for byte and boot0 beside it from the
#          artifacts the last OS build left — the kernel and the system bundles — and a data
#          seed made here the way the QEMU launcher makes its own; `nx image flash-plan` turns
#          the disk into the raw regions `scripts/board-flash.sh --plan` writes. The vendor boot
#          pieces are checked against their pins first. Everything lands in build/board/<board>/.
# OWNERS:  @tools-team @reliability
# STATUS:  Functional
# API_STABILITY: Stable (flags)
# TEST_COVERAGE: tools/nx/tests/image_board_cli.rs, image_flash_cli.rs (the builder and the plan);
#                TASK-0260 P2 flashed its output and read it back on the desk board
# DEPENDS_ON: cargo (the host `nx`), scripts/fetch-board-inputs.sh, the OS build's artifacts
#
# Usage:
#   scripts/board-image.sh              # build/board/bpi-f3/{nexus.img, nexus.img.boot0, flash/}
#   scripts/board-image.sh --fit FILE   # ... with the loader's FIT in `uboot` (TASK-0260B)
#
# Exit codes: 0 built · 1 inputs missing · 2 a build step failed

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BOARD="bpi-f3"
OUT="$ROOT/build/board/$BOARD"
TARGET_ROOT="${CARGO_TARGET_DIR:-$ROOT/target}"
KERNEL="$TARGET_ROOT/riscv64imac-unknown-none-elf/release/neuron-boot.bin"
BUNDLES="$ROOT/build/system-bundles"
SIGN="$ROOT/keys/dev-os-image.ed25519.seed"
PUBLISHER="$ROOT/keys/dev-publisher.ed25519.seed"

FIT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --fit)     FIT="${2:?--fit needs a file}"; shift ;;
    -h|--help) sed -n '18,21p' "$0"; exit 0 ;;
    *) echo "[error] unknown flag: $1" >&2; exit 1 ;;
  esac
  shift
done

log() { printf '\033[1;34m[board-image]\033[0m %s\n' "$*"; }
err() { printf '\033[1;31m[board-image][error]\033[0m %s\n' "$*" >&2; }

"$ROOT/scripts/fetch-board-inputs.sh" --check >/dev/null 2>&1 || {
  err "vendor boot pieces missing or not matching their pins — run: just board-inputs"; exit 1; }
[ -f "$KERNEL" ] || { err "no kernel at $KERNEL — run: just build-os-workspace"; exit 1; }
[ -d "$BUNDLES" ] || { err "no system bundles at $BUNDLES — run: just build-os-workspace"; exit 1; }
[ -z "$FIT" ] || [ -f "$FIT" ] || { err "no FIT at $FIT"; exit 1; }

# The HOST nx: the OS RUSTFLAGS (nexus_env=os) must not leak in (as the launcher builds it).
(cd "$ROOT" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS cargo build --release -p nx >/dev/null) || {
  err "building nx failed"; exit 2; }
NX="$TARGET_ROOT/release/nx"
mkdir -p "$OUT"
build_id="dev-$(sha256sum "$KERNEL" | cut -c1-8)"

log "data seed (the launcher's fixture set)"
"$NX" image fixtures --kernel "$KERNEL" --data-out "$OUT/data-seed.img" \
  --sign-os "$SIGN" --sign-publisher "$PUBLISHER" --build-id "$build_id" >/dev/null || {
  err "nx image fixtures failed"; exit 2; }

fit=()
[ -z "$FIT" ] || fit=(--fit "$FIT")
log "disk for $BOARD (build $build_id${FIT:+, FIT $FIT})"
"$NX" image build --target "$BOARD" --kernel "$KERNEL" --out "$OUT/nexus.img" \
  --sign "$SIGN" --build-id "$build_id" --rollback-index 1 \
  --data "$OUT/data-seed.img" --system-bundles "$BUNDLES" "${fit[@]}" >/dev/null || {
  err "nx image build failed"; exit 2; }

rm -rf "$OUT/flash"
"$NX" image flash-plan --image "$OUT/nexus.img" --out-dir "$OUT/flash" >/dev/null || {
  err "nx image flash-plan failed"; exit 2; }
log "flash plan: $OUT/flash/plan.json — write it with: just board-flash --plan $OUT/flash"
