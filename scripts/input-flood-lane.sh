#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: the input chain survives a real drag (TASK-0054C P2-g).
#
# Every other lane drives input politely — a few pointer moves, a click, a key —
# and that politeness hid a defect a user found in seconds: inputd allocated one
# `Vec` per HID batch on an os-lite heap that never frees, so ~400 batches a
# second walked its 384 KiB heap to `alloc_error` in under twenty seconds. A dead
# inputd stops consuming, hidrawd's sends back up (`tx hz=0` while events keep
# arriving), and input is gone fleet-wide — which is why the reported boot sat on
# the greeter with a login nobody could click.
#
# This lane boots the visible profile, waits for the harness' own injector to
# finish, then floods absolute pointer moves at the rate a drag produces, and
# asserts what must NOT happen:
#   * no `alloc-fail` / `alloc_error` from any service, and
#   * hidrawd still forwarding at the end (`tx hz` non-zero while `rx hz` is).
#
# Limits, stated: this is a LOAD proof, not a latency one, and it needs QMP
# (`QEMU_INPUT_AUTOINJECT=1`), so it runs in the visible profile only.
set -euo pipefail

cd "$(dirname "$0")/.."

FLOOD_SECONDS=${FLOOD_SECONDS:-45}
FLOOD_RATE=${FLOOD_RATE:-900}
SOCKET=build/qemu.qmp
fail=0

rm -f "$SOCKET"
RUN_TIMEOUT=${RUN_TIMEOUT:-150s} QEMU_INPUT_AUTOINJECT=1 just test-os visible &
lane=$!

# Wait for the SOCKET, never for a marker in `build/logs/latest` — that symlink
# still points at the previous run while this one builds.
for _ in $(seq 1 900); do
  [ -S "$SOCKET" ] && break
  sleep 1
done
if [ ! -S "$SOCKET" ]; then
  echo "[FAIL] input-flood: QMP socket never appeared" >&2
  kill "$lane" 2>/dev/null || true
  exit 1
fi

# QMP accepts ONE client: the harness' injector connects first for its visible
# proof sequence. Take the socket only once it has exited.
for _ in $(seq 1 180); do
  pgrep -f qmp_visible_input_inject.py > /dev/null 2>&1 || break
  sleep 1
done
sleep 2

python3 tools/qmp_input_flood.py "$SOCKET" "$FLOOD_SECONDS" "$FLOOD_RATE" || {
  echo "[FAIL] input-flood: the flood itself failed" >&2
  fail=1
}
wait "$lane" || true

UART=$(ls -dt build/logs/visible--*/uart.log 2>/dev/null | head -1)
[ -n "$UART" ] || { echo "[FAIL] input-flood: no uart log" >&2; exit 1; }
echo "[info] input-flood: $UART"

if grep -qE 'alloc-fail|alloc_error' "$UART"; then
  echo "[FAIL] input-flood: a service ran out of heap under input load:" >&2
  grep -E 'alloc-fail|alloc_error' "$UART" | head -5 >&2
  fail=1
fi

# The wedge's signature: events still arriving, nothing forwarded.
if grep -E 'hidrawd: wake hz' "$UART" | tail -3 | grep -qE 'rx hz=[1-9][0-9]* .* tx hz=0'; then
  echo "[FAIL] input-flood: hidrawd stopped forwarding while events kept arriving:" >&2
  grep -E 'hidrawd: wake hz' "$UART" | tail -3 >&2
  fail=1
fi

if [ "$fail" == "0" ]; then
  echo "[PASS] input-flood: input chain survived ${FLOOD_SECONDS}s at ~${FLOOD_RATE}/s (no heap exhaustion, forwarding intact)"
  grep -E 'hidrawd: wake hz' "$UART" | tail -2
fi
exit "$fail"
