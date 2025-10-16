#!/usr/bin/env bash
set -euo pipefail

echo "[postflight] idl & symbols"
rg -n 'getPayload|OPCODE_GET_PAYLOAD' source -g '!**/target/**' >/dev/null
rg -n 'as_create\(|as_map\(|spawn\(' source/libs/nexus-abi/src -g '!**/target/**' >/dev/null

echo "[postflight] host loader tests"
cargo test -p nexus-loader -- --nocapture

echo "[postflight] qemu (bounded, early-exit)"
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=${RUN_TIMEOUT:-60s} just test-os

echo "[postflight] check exec markers"
rg -n 'execd: elf load ok' uart.log >/dev/null
rg -n 'child: hello-elf'   uart.log >/dev/null
rg -n 'SELFTEST: e2e exec-elf ok' uart.log >/dev/null

echo "[postflight] OK"
