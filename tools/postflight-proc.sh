#!/usr/bin/env bash
set -euo pipefail

echo "[postflight] build workspace"
cargo build --workspace

echo "[postflight] qemu (bounded, early-exit)"
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=${RUN_TIMEOUT:-60s} just test-os

echo "[postflight] kernel markers"
rg -n 'KSELFTEST: exit ok' uart.log >/dev/null
rg -n 'KSELFTEST: wait ok' uart.log >/dev/null

echo "[postflight] os markers"
rg -n 'SELFTEST: child exit ok' uart.log >/dev/null

echo "[postflight] OK"
