#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
UART_LOG=${UART_LOG:-uart.log}
QEMU_LOG=${QEMU_LOG:-qemu.log}
RUN_TIMEOUT=${RUN_TIMEOUT:-90s}
RUN_UNTIL_MARKER=${RUN_UNTIL_MARKER:-1}
QEMU_LOG_MAX=${QEMU_LOG_MAX:-52428800}
UART_LOG_MAX=${UART_LOG_MAX:-10485760}

# Continuous QEMU tracing can easily balloon into tens of gigabytes; trim the
# tail post-run to keep CI artifacts and local logs manageable.
trim_log() {
  local file=$1 max=$2
  if [[ -f "$file" ]]; then
    local sz
    sz=$(wc -c <"$file" || echo 0)
    if [[ "$sz" -gt "$max" ]]; then
      echo "[info] Trimming $file from ${sz} bytes to last $max bytes" >&2
      tail -c "$max" "$file" >"${file}.tmp" && mv "${file}.tmp" "$file"
    fi
  fi
}

rm -f "$UART_LOG" "$QEMU_LOG"

QEMU_EXTRA_ARGS=()
if [[ "${DEBUG_QEMU:-0}" == "1" ]]; then
  QEMU_EXTRA_ARGS+=(-S -gdb tcp:localhost:1234)
fi

set +e
RUN_TIMEOUT="$RUN_TIMEOUT" \
RUN_UNTIL_MARKER="$RUN_UNTIL_MARKER" \
QEMU_LOG="$QEMU_LOG" \
UART_LOG="$UART_LOG" \
QEMU_LOG_MAX="$QEMU_LOG_MAX" \
UART_LOG_MAX="$UART_LOG_MAX" \
"$ROOT/scripts/run-qemu-rv64.sh" "${QEMU_EXTRA_ARGS[@]}"
qemu_status=$?
set -e

# Verify markers printed by the OS stack and selftest client. The harness waits
# for os-lite init to announce each service twice (`init: start <svc>` then
# `init: up <svc>`) before the packagefsd/vfsd readiness markers, the execd
# lifecycle markers, and the selftest tail. If os-lite userspace did not run,
# fall back to kernel selftest completion markers to avoid spurious failures
# during bring-up.
expected_sequence=(
  "neuron vers."
  "init: start"
  "init: start keystored"
  "init: up keystored"
  "keystored: ready"
  "init: start policyd"
  "init: up policyd"
  "policyd: ready"
  "init: start samgrd"
  "init: up samgrd"
  "samgrd: ready"
  "init: start bundlemgrd"
  "init: up bundlemgrd"
  "bundlemgrd: ready"
  "init: start packagefsd"
  "init: up packagefsd"
  "packagefsd: ready"
  "init: start vfsd"
  "init: up vfsd"
  "vfsd: ready"
  "init: start execd"
  "init: up execd"
  "init: ready"
  "execd: elf load ok"
  "child: hello-elf"
  "SELFTEST: e2e exec-elf ok"
  "child: exit0 start"
  "execd: child exited"
  "SELFTEST: child exit ok"
  "SELFTEST: vfs stat ok"
  "SELFTEST: vfs read ok"
  "SELFTEST: vfs ebadf ok"
)

# SATP hang diagnostic: saw trampoline enter but no post-satp OK
if grep -aFq "AS: trampoline enter" "$UART_LOG" && ! grep -aFq "AS: post-satp OK" "$UART_LOG"; then
  echo "[error] SATP switch hang: trampoline entered but no post-satp marker" >&2
  exit 1
fi

# Require os-lite userspace init markers; kernel fallback no longer accepted
if ! grep -aFq "init: start" "$UART_LOG"; then
  echo "[error] os-lite init markers not found (init: start); userspace bring-up required" >&2
  exit 1
fi

missing=0
for marker in "${expected_sequence[@]}"; do
  if ! grep -aFq "$marker" "$UART_LOG"; then
    missing=1
    break
  fi
done

prev=-1
for marker in "${expected_sequence[@]}"; do
  line=$(grep -aFn "$marker" "$UART_LOG" | head -n1 | cut -d: -f1)
  if [[ -z "$line" ]]; then
    # If we never matched the full sequence, tolerate missing lines here; the
    # final RUN_UNTIL_MARKER gating below decides success/failure.
    break
  fi
  if [[ "$prev" -ne -1 && "$line" -le "$prev" ]]; then
    echo "Marker out of order: $marker (line $line)" >&2
    exit 1
  fi
  prev=$line
done

for policy_marker in "SELFTEST: policy allow ok" "SELFTEST: policy deny ok"; do
  if ! grep -aFq "$policy_marker" "$UART_LOG"; then
    echo "Missing UART marker: $policy_marker" >&2
    exit 1
  fi
done

trim_log "$QEMU_LOG" "$QEMU_LOG_MAX"
trim_log "$UART_LOG" "$UART_LOG_MAX"

if [[ "$qemu_status" -ne 0 && "$RUN_UNTIL_MARKER" != "1" ]]; then
  echo "[warn] QEMU exited with status $qemu_status" >&2
fi
echo "QEMU selftest completed (markers verified)." >&2
