#!/usr/bin/env bash
set -euo pipefail

echo "[postflight] build workspace"
cargo build --workspace

echo "[postflight] host vfs tests"
cargo test -p vfs-e2e -- --nocapture

echo "[postflight] qemu run (bounded, early-exit)"
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=${RUN_TIMEOUT:-60s} just test-os

echo "[postflight] check OS vfs markers"
rg -n 'packagefsd: ready' uart.log >/dev/null
rg -n 'vfsd: ready'      uart.log >/dev/null
rg -n 'SELFTEST: vfs stat ok' uart.log >/dev/null
rg -n 'SELFTEST: vfs read ok' uart.log >/dev/null
rg -n 'SELFTEST: vfs ebadf ok' uart.log >/dev/null

echo "[postflight] OK"
