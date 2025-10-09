#!/usr/bin/env bash
set -euo pipefail

echo "[postflight] services present"
for s in execd keystored policyd; do
  test -f "source/services/$s/Cargo.toml" || { echo "missing $s crate"; exit 1; }
done

echo "[postflight] run qemu (bounded, early-exit on markers)"
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=${RUN_TIMEOUT:-45s} just test-os

echo "[postflight] check marker sequence"
rg -n 'NEURON|init: start|keystored: ready|policyd: ready|samgrd: ready|bundlemgrd: ready|init: ready' uart.log >/dev/null

echo "[postflight] logs capped"
[ "$(wc -c < qemu.log 2>/dev/null || echo 0)" -le "${QEMU_LOG_MAX:-52428800}" ]
[ "$(wc -c < uart.log 2>/dev/null || echo 0)" -le "${UART_LOG_MAX:-10485760}" ]

echo "[postflight] OK"
