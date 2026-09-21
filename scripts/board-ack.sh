#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: A human at the monitor confirms what no readback can prove yet
#          (the desktop on the HDMI monitor, typed input arriving) — and the
#          confirmation lands IN the board log as a marker, `board-visual:
#          <name>`, so the proof manifest can require it like any other line
#          (TASK-0327 / TASK-0327B). Prose in a ledger is not a proof; a marker
#          in the log of the run it describes is. Names are declared in the
#          manifest; this script refuses anything else so a typo cannot pass.
# OWNERS:  @tools-team
# STATUS:  Functional
# API_STABILITY: Stable (marker literal `board-visual: <name>`)
# TEST_COVERAGE: TASK-0327B lane
# DEPENDS_ON: a current capture (build/logs/latest-board/uart.log)
#
# Usage:
#   scripts/board-ack.sh <name>          # append "board-visual: <name>" to the latest board log
#   scripts/board-ack.sh --list          # print the names the manifest declares
#
# Exit codes: 0 appended · 1 unknown name / no current log

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST_DIR="$ROOT/source/apps/selftest-client/proof-manifest/markers"

declared() {
  # Every `[marker."board-visual: <name>"]` across the manifest files.
  grep -rhoE '^\[marker\."board-visual: [a-z0-9-]+"\]' "$MANIFEST_DIR" 2>/dev/null \
    | sed -E 's/^\[marker\."board-visual: ([a-z0-9-]+)"\]$/\1/' | sort -u
}

case "${1:-}" in
  --list) declared; exit 0 ;;
  -h|--help|"") sed -n '19,23p' "$0"; exit 1 ;;
esac
name="$1"

if ! declared | grep -qx "$name"; then
  echo "[board] '$name' is not a declared board-visual marker. Declared:" >&2
  declared | sed 's/^/  - /' >&2
  echo "[board] declare it in $MANIFEST_DIR/board.toml first (a marker is a contract)." >&2
  exit 1
fi

log="$ROOT/build/logs/latest-board/uart.log"
if [ ! -f "$log" ]; then
  echo "[board] no current board log ($log) — start 'just board-serial' first" >&2
  exit 1
fi
printf 'board-visual: %s\n' "$name" >>"$log"
echo "[board] acknowledged: board-visual: $name  → $log"
