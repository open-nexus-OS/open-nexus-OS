#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
UART_LOG=${UART_LOG:-uart.log}
QEMU_LOG=${QEMU_LOG:-qemu.log}
TARGET=${TARGET:-riscv64imac-unknown-none-elf}

rm -f "$UART_LOG" "$QEMU_LOG"

QEMU_ARGS=(-d int,mmu,unimp -D "$QEMU_LOG")
if [[ "${DEBUG_QEMU:-0}" == "1" ]]; then
  QEMU_ARGS+=(-S -gdb tcp:localhost:1234)
fi

# Build the kernel first so QEMU has an image to boot.
if [[ ! -f "$ROOT/target/$TARGET/debug/libneuron.a" ]]; then
  (cd "$ROOT" && cargo build -p neuron --target "$TARGET")
fi

# Run QEMU and tee the UART output to a log file for post-processing.
"$ROOT/scripts/run-qemu-rv64.sh" "${QEMU_ARGS[@]}" | tee "$UART_LOG"

# Verify expected boot markers are present in the UART log.
required_markers=(
  "NEURON"
  "boot: ok"
  "traps: ok"
  "sys: ok"
  "SELFTEST: begin"
  "SELFTEST: time ok"
  "SELFTEST: ipc ok"
  "SELFTEST: caps ok"
  "SELFTEST: map ok"
  "SELFTEST: sched ok"
  "SELFTEST: end"
)

for marker in "${required_markers[@]}"; do
  if ! grep -Fq "$marker" "$UART_LOG"; then
    echo "Missing UART marker: $marker" >&2
    exit 1
  fi
done

echo "QEMU selftest completed successfully." >&2
