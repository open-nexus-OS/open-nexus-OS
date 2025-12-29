#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

# NOTE: Deprecated as a "proof" definition.
# This script can keep extra host-side build checks, but QEMU proof must come from:
#   - scripts/qemu-test.sh
#
# Do NOT add uart.log greps here. If markers/ordering change, update scripts/qemu-test.sh instead.

RUN_TIMEOUT=${RUN_TIMEOUT:-60s}
RUN_UNTIL_MARKER=${RUN_UNTIL_MARKER:-1}

echo "[postflight] host build (exclude kernel)"
RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="host"' \
  cargo build --workspace --exclude neuron --exclude neuron-boot

echo "[postflight] qemu proof (delegated)"
RUN_TIMEOUT="$RUN_TIMEOUT" RUN_UNTIL_MARKER="$RUN_UNTIL_MARKER" "$ROOT/scripts/qemu-test.sh"

echo "[postflight] completed (delegated to scripts/qemu-test.sh)"
