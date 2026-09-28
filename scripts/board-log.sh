#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: The board's boot trace, read without a serial adapter (RFC-0107, TASK-0327B P1).
#          Every boot from our eMMC leaves its console text in the disk's `trace` partition
#          (the loader's region today; the OS region with Phase 2). The board's stock system —
#          on the microSD, which the board boots first (DIP 1+2 default) — reads it back over
#          adb: the partition is found by its GPT name as the stock kernel parsed it, its size
#          checked against the layout's 8 MiB, pulled binary-safe (`adb exec-out dd`) and read
#          by `nx image trace`, which counts a boot only with its magic, version, CRC and
#          in-bounds lengths. The latest boot becomes build/logs/board--<ts>/uart.log — the file
#          the marker tools read, as they read a serial capture — every kept boot
#          trace-all.log, the partition itself trace.part; `build/logs/latest-board` points at it.
# OWNERS:  @tools-team @runtime
# STATUS:  Functional
# API_STABILITY: Stable (exit codes)
# TEST_COVERAGE: the reader: tools/nx/tests/image_trace_cli.rs; the pull: TASK-0327B against
#                the desk board
# DEPENDS_ON: adb (android-tools), cargo (the host `nx`), python3
#
# Usage:
#   scripts/board-log.sh     # the latest boot → build/logs/board--<ts>/uart.log (+ trace-all.log)
#
# Exit codes: 0 read · 1 tool missing · 2 the pull or the read failed · 3 no stock system on adb ·
#             4 the eMMC has no trace partition (not our disk, or one built before RFC-0107) ·
#             5 the trace keeps no boot (since the disk was written, no boot reached the loader's
#               disk step: the chain stopped before nxboot, or nxboot found no boot disk)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_ROOT="${CARGO_TARGET_DIR:-$ROOT/target}"
# The eMMC as the stock system names it (mmc2; measured 2026-09-22).
STOCK_EMMC="mmcblk2"
# The layout's trace partition: eight 1 MiB slots (storage::trace).
TRACE_SECTORS=16384

case "${1:-}" in
  "") ;;
  -h|--help) sed -n '21,22p' "$0"; exit 0 ;;
  *) echo "[error] unknown flag: $1" >&2; exit 1 ;;
esac

log() { printf '\033[1;34m[board-log]\033[0m %s\n' "$*"; }
err() { printf '\033[1;31m[board-log][error]\033[0m %s\n' "$*" >&2; }

command -v adb >/dev/null 2>&1 || { err "adb missing — scripts/install-deps.sh (android-tools)"; exit 1; }
[ "$(timeout 10 adb get-state 2>/dev/null)" = "device" ] || {
  err "no stock system on adb — insert the microSD, reset the board, wait for it to boot, re-run"; exit 3; }

# The partition by its GPT name, as the stock kernel parsed our GPT: "<dev> <start> <sectors>".
# stdin from /dev/null: `adb shell` reads it otherwise.
found="$(timeout 20 adb shell "for p in /sys/block/$STOCK_EMMC/${STOCK_EMMC}p*; do \
  [ -e \"\$p/uevent\" ] || continue; \
  echo \"\${p##*/} \$(cat \$p/start) \$(cat \$p/size) \$(sed -n 's/^PARTNAME=//p' \$p/uevent)\"; done" \
  </dev/null | tr -d '\r' | awk '$4 == "trace" { print $1, $2, $3 }')"
[ -n "$found" ] || {
  err "the eMMC ($STOCK_EMMC) has no trace partition — flash a disk built since RFC-0107: just board-image; just board-flash --plan build/board/bpi-f3/flash"
  exit 4; }
[ "$(printf '%s\n' "$found" | wc -l)" -eq 1 ] || { err "more than one trace partition on the eMMC: $found"; exit 2; }
read -r dev start sectors <<<"$found"
[ "$sectors" = "$TRACE_SECTORS" ] || {
  err "/dev/$dev (@ $start) has $sectors sectors, the layout's trace partition has $TRACE_SECTORS"; exit 2; }

ts="$(date +%Y-%m-%dT%H-%M-%S)"
dir="$ROOT/build/logs/board--$ts"
mkdir -p "$dir"
log "pulling /dev/$dev (lba $start, $sectors sectors) → $dir/trace.part"
# exec-out: a binary-safe stream (no pty, no line-ending translation); dd's own chatter stays remote.
if ! timeout 120 adb exec-out "dd if=/dev/$dev bs=1M count=8 2>/dev/null" >"$dir/trace.part" </dev/null; then
  err "the pull failed"; exit 2
fi
got="$(stat -c %s "$dir/trace.part")"
[ "$got" = "$((TRACE_SECTORS * 512))" ] || { err "pulled $got bytes, the partition has $((TRACE_SECTORS * 512))"; exit 2; }

# The HOST nx: the OS RUSTFLAGS (nexus_env=os) must not leak in (as the launcher builds it).
(cd "$ROOT" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS cargo build --release -p nx >/dev/null 2>&1) || {
  err "building nx failed"; exit 2; }
NX="$TARGET_ROOT/release/nx"

set +e
meta="$("$NX" image trace --image "$dir/trace.part" --out "$dir/uart.log" --json)"
rc=$?
set -e
if [ "$rc" = 3 ] && grep -q "keeps no boot" <<<"$meta"; then
  err "the trace keeps no boot: since the disk was written, no boot reached the loader's disk step — the chain stopped before nxboot, or nxboot found no boot disk (its console is the only witness before that step)"
  exit 5
fi
[ "$rc" = 0 ] || { err "nx image trace failed ($rc): $meta"; exit 2; }
"$NX" image trace --image "$dir/trace.part" --all --out "$dir/trace-all.log" >/dev/null
ln -sfn "board--$ts" "$ROOT/build/logs/latest-board"

python3 - "$meta" <<'PY'
import json, sys
data = json.loads(sys.argv[1])["data"]
b = data["boots"][-1]
def region(name):
    if b[f"{name}_bytes"] == 0 and not b[f"{name}_complete"]:
        return "nothing kept"
    state = "complete" if b[f"{name}_complete"] else "INCOMPLETE"
    if b[f"{name}_overflow"]:
        state += ", overflowed"
    if b.get(f"{name}_rescued"):
        state += ", rescued from RAM by the next loader"
    return f"{b[f'{name}_bytes']} bytes ({state})"
print(f"[board-log] {data['kept']} boot(s) kept; the latest: seq {b['seq']} (slot {b['slot']})")
print(f"[board-log]   loader: {region('loader')}")
print(f"[board-log]   OS:     {region('os')}")
PY
log "the latest boot's text ($dir/uart.log):"
tail -n 60 "$dir/uart.log" | sed 's/^/    /'
