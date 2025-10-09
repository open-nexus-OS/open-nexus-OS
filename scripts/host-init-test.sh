#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
INIT_LOG=${INIT_LOG:-host-init.log}
RUN_TIMEOUT=${RUN_TIMEOUT:-30s}

rm -f "$INIT_LOG"

monitor_init() {
  local line
  while IFS= read -r line; do
    case "$line" in
      *"init: ready"*)
        echo "[info] init ready marker detected – stopping nexus-init" >&2
        pkill -f nexus-init >/dev/null 2>&1 || true
        break
        ;;
    esac
  done
}

set +e
timeout --foreground "$RUN_TIMEOUT" \
  stdbuf -oL env RUSTFLAGS='--cfg nexus_env="host"' \
  cargo run -q -p nexus-init \
  | tee >(monitor_init) \
  | tee "$INIT_LOG"
status=${PIPESTATUS[0]}
set -e

if [[ "$status" -ne 0 && "$status" -ne 143 ]]; then
  echo "[error] nexus-init exited with status $status" >&2
  exit "$status"
fi

# Enforce readiness marker order
expected_sequence=(
  "init: start"
  "keystored: ready"
  "policyd: ready"
  "samgrd: ready"
  "bundlemgrd: ready"
  "init: ready"
)

missing=0
for marker in "${expected_sequence[@]}"; do
  if ! grep -aFq "$marker" "$INIT_LOG"; then
    echo "Missing marker: $marker" >&2
    missing=1
  fi
done
[[ "$missing" -eq 0 ]] || exit 1

prev=-1
for marker in "${expected_sequence[@]}"; do
  line=$(grep -aFn "$marker" "$INIT_LOG" | head -n1 | cut -d: -f1)
  if [[ -z "$line" ]]; then
    echo "Marker not found for ordering check: $marker" >&2
    exit 1
  fi
  if [[ "$prev" -ne -1 && "$line" -le "$prev" ]]; then
    echo "Marker out of order: $marker (line $line)" >&2
    exit 1
  fi
  prev=$line
done

# Also require *: up confirmations (presence only)
for svc in keystored policyd samgrd bundlemgrd; do
  if ! grep -aFq "$svc: up" "$INIT_LOG"; then
    echo "Missing up confirmation: $svc: up" >&2
    exit 1
  fi
done

echo "[info] host init test succeeded" >&2
