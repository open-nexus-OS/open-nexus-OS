#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

# NOTE: Deprecated as a "proof" definition.
# QEMU/marker proof must come from:
#   - scripts/qemu-test.sh
#
# Do NOT add uart.log greps here. If markers/ordering change, update scripts/qemu-test.sh instead.

RUN_TIMEOUT=${RUN_TIMEOUT:-45s}
RUN_UNTIL_MARKER=${RUN_UNTIL_MARKER:-1}

RUN_TIMEOUT="$RUN_TIMEOUT" RUN_UNTIL_MARKER="$RUN_UNTIL_MARKER" "$ROOT/scripts/qemu-test.sh"
echo "[postflight-kspawn] completed (delegated to scripts/qemu-test.sh)"
