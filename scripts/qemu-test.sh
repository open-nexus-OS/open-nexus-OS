#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
UART_LOG=${UART_LOG:-uart.log}
QEMU_LOG=${QEMU_LOG:-qemu.log}
RUN_TIMEOUT=${RUN_TIMEOUT:-45s}
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

RUN_TIMEOUT="$RUN_TIMEOUT" \
RUN_UNTIL_MARKER="$RUN_UNTIL_MARKER" \
QEMU_LOG="$QEMU_LOG" \
UART_LOG="$UART_LOG" \
QEMU_LOG_MAX="$QEMU_LOG_MAX" \
UART_LOG_MAX="$UART_LOG_MAX" \
"$ROOT/scripts/run-qemu-rv64.sh" "${QEMU_EXTRA_ARGS[@]}"

# Verify markers. If init markers are present, enforce strict order; otherwise
# accept a kernel-only run with the selftest success marker.
if grep -aFq "init: start" "$UART_LOG"; then
  expected_sequence=(
    "neuron vers."
    "init: start"
    "keystored: ready"
    "policyd: ready"
    "samgrd: ready"
    "bundlemgrd: ready"
    "init: ready"
    "execd: elf load ok"
    "child: hello-elf"
    "SELFTEST: e2e exec-elf ok"
  )

  missing=0
  for marker in "${expected_sequence[@]}"; do
    if ! grep -aFq "$marker" "$UART_LOG"; then
      echo "Missing UART marker: $marker" >&2
      missing=1
    fi
  done
  if [[ "$missing" -ne 0 ]]; then
    exit 1
  fi

  prev=-1
  for marker in "${expected_sequence[@]}"; do
    line=$(grep -aFn "$marker" "$UART_LOG" | head -n1 | cut -d: -f1)
    if [[ -z "$line" ]]; then
      echo "Marker not found for ordering check: $marker" >&2
      exit 1
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
else
  # Kernel-only mode: enforce banner and selftest completion
  if ! grep -aFq "neuron vers." "$UART_LOG"; then
    echo "Missing UART marker: neuron vers." >&2
    exit 1
  fi
  if ! grep -aFq "I: after selftest" "$UART_LOG"; then
    echo "Missing UART marker: I: after selftest" >&2
    exit 1
  fi
  # Optional signed install markers (best-effort)
  if grep -aFq "SELFTEST: signed install ok" "$UART_LOG"; then
    echo "[info] Signed install selftest succeeded" >&2
  fi
fi

trim_log "$QEMU_LOG" "$QEMU_LOG_MAX"
trim_log "$UART_LOG" "$UART_LOG_MAX"

echo "QEMU selftest completed successfully." >&2
