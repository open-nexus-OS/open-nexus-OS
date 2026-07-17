#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Reconcile the chain-marker contract (tools/nx/chains/markers.txt)
#          against a real QEMU uart.log — the bridge between the simulated
#          chain tests and the real boot.
# OWNERS: @tools-team
# STATUS: Functional
# API_STABILITY: Stable
# TEST_COVERAGE: exercised by qemu-test.sh proof profiles (MARKER_CONTRACT=1)
# ADR: docs/adr/0030-integration-chain-test-framework.md
#
# Usage: check-chain-markers.sh [--log <uart.log>] [--groups a,b,c]
#   --log     uart log to check (default: build/logs/latest/uart.log)
#   --groups  comma-separated group names from markers.txt headers
#             (default: input,gpu,display)

set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTRACT="$REPO_ROOT/tools/nx/chains/markers.txt"
LOG="$REPO_ROOT/build/logs/latest/uart.log"
WANT_GROUPS="input,gpu,display"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --log) LOG=$2; shift 2 ;;
    --groups) WANT_GROUPS=$2; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

[[ -f "$CONTRACT" ]] || { echo "[error] contract missing: $CONTRACT" >&2; exit 2; }
[[ -f "$LOG" ]] || { echo "[error] uart log missing: $LOG" >&2; exit 2; }

miss=0 total=0 group="" active=0
while IFS= read -r line; do
  case "$line" in
    "# group: "*)
      group="${line#\# group: }"; group="${group%% *}"
      case ",$WANT_GROUPS," in *",$group,"*) active=1 ;; *) active=0 ;; esac
      continue ;;
    "#"*|"") continue ;;
  esac
  [[ "$active" == "1" ]] || continue
  total=$(( total + 1 ))
  if grep -aFq -- "$line" "$LOG"; then
    echo "[ok]   $line"
  else
    echo "[MISS] $line"
    miss=$(( miss + 1 ))
  fi
done < "$CONTRACT"

if [[ "$miss" -gt 0 ]]; then
  echo "[FAIL] chain-marker contract: $miss/$total markers missing in $LOG" >&2
  exit 1
fi
echo "[PASS] chain-marker contract: $total/$total markers present ($WANT_GROUPS) in $LOG"
