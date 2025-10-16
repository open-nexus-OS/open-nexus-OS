#!/usr/bin/env bash
set -euo pipefail

echo "[postflight] host build (exclude kernel)"
RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="host"' \
  cargo build --workspace --exclude neuron --exclude neuron-boot

echo "[postflight] qemu (bounded, early-exit)"
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=${RUN_TIMEOUT:-60s} just test-os

echo "[postflight] check minimal exec markers"
rg -n 'execd: spawn ok' uart.log >/dev/null
rg -n 'child: hello-elf'   uart.log >/dev/null
rg -n 'SELFTEST: e2e exec ok' uart.log >/dev/null

echo "[postflight] OK"
