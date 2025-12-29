#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

# NOTE: Deprecated as a "proof" definition.
# QEMU/marker proof must come from:
#   - scripts/qemu-test.sh
#
# Do NOT add uart.log greps here. If markers/ordering change, update scripts/qemu-test.sh instead.

RUN_TIMEOUT=${RUN_TIMEOUT:-60s}
RUN_UNTIL_MARKER=${RUN_UNTIL_MARKER:-1}

echo "[postflight] recipes/policy present"
test -d recipes/policy
test -f recipes/policy/base.toml

echo "[postflight] host e2e policy"
cargo test -p e2e_policy -- --nocapture

echo "[postflight] qemu proof (delegated)"
RUN_TIMEOUT="$RUN_TIMEOUT" RUN_UNTIL_MARKER="$RUN_UNTIL_MARKER" "$ROOT/scripts/qemu-test.sh"

echo "[postflight] completed (delegated to scripts/qemu-test.sh)"
