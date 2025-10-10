#!/usr/bin/env bash
set -euo pipefail

echo "[postflight] recipes/policy present"
test -d recipes/policy
test -f recipes/policy/base.toml

echo "[postflight] host e2e policy"
cargo test -p e2e_policy -- --nocapture

echo "[postflight] qemu run (bounded, early-exit)"
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=${RUN_TIMEOUT:-60s} just test-os

echo "[postflight] check OS policy markers"
rg -n 'SELFTEST: policy allow ok' uart.log >/dev/null
rg -n 'SELFTEST: policy deny ok'  uart.log >/dev/null

echo "[postflight] OK"
