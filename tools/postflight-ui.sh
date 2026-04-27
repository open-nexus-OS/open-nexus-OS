#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

if [[ "${1:-}" == "--uart-log" || "${1:-}" == "--log-only" ]]; then
  echo "[error] UI postflight refuses log-grep-only closure; run the canonical QEMU proof." >&2
  exit 2
fi

export RUN_UNTIL_MARKER="${RUN_UNTIL_MARKER:-1}"
export RUN_TIMEOUT="${RUN_TIMEOUT:-190s}"
export PM_VERIFY_UART="${PM_VERIFY_UART:-1}"

exec "$ROOT/scripts/qemu-test.sh" "$@"
