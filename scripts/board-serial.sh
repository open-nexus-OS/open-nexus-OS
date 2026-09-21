#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: The board's debug console, recorded like a QEMU run (TASK-0327).
#          Opens the USB-UART adapter at 115200 8N1 and tees everything into
#          build/logs/board--<ts>/uart.log, so the marker tools that read a
#          QEMU uart.log (`just check-markers`, verify-uart, the FAIL gate)
#          read a board log without knowing the difference. `build/logs/
#          latest-board` points at the newest capture; `just board-ack` appends
#          to it. Interactive by default (picocom; quit with C-a C-x); with
#          --capture it only records, for the proof lane (TASK-0327B).
# OWNERS:  @tools-team
# STATUS:  Functional
# API_STABILITY: Stable
# TEST_COVERAGE: TASK-0327 T2 (manual, against the desk board); TASK-0327B lane
# DEPENDS_ON: picocom (interactive), stty + cat (capture)
#
# Usage:
#   scripts/board-serial.sh [PORT]            # interactive picocom + log tee
#   scripts/board-serial.sh --capture [PORT]  # record only (no input), until killed
#   BOARD_BAUD=115200                         # override the baud rate
#
# Exit codes:
#   0  session ended normally
#   3  no serial adapter found
#   1  usage / tool missing

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BAUD="${BOARD_BAUD:-115200}"
CAPTURE=0
PORT=""
for arg in "$@"; do
  case "$arg" in
    --capture) CAPTURE=1 ;;
    -h|--help) sed -n '20,29p' "$0"; exit 0 ;;
    -*) echo "[error] unknown flag: $arg" >&2; exit 1 ;;
    *) PORT="$arg" ;;
  esac
done

if [ -z "$PORT" ]; then
  PORT="$("$ROOT/scripts/board-devices.sh" --serial)"
fi
if [ -z "$PORT" ] || [ ! -e "$PORT" ]; then
  echo "[board] no serial adapter found — plug a 3.3 V TTL USB-UART into UART0 (GND/RX/TX), or pass the port" >&2
  exit 3
fi
if [ ! -r "$PORT" ] || [ ! -w "$PORT" ]; then
  echo "[board] $PORT is not accessible — serial group membership missing or not effective yet (scripts/install-board-access.sh, then log out and in)" >&2
  exit 1
fi

ts="$(date +%Y-%m-%dT%H-%M-%S)"
dir="$ROOT/build/logs/board--$ts"
mkdir -p "$dir"
log="$dir/uart.log"
ln -sfn "board--$ts" "$ROOT/build/logs/latest-board"
echo "[board] console $PORT @ $BAUD → $log"

if [ "$CAPTURE" = 1 ]; then
  # Record only: raw mode, no echo, no line discipline surprises. `cat` ends
  # when the caller kills us (the proof lane owns the lifetime).
  stty -F "$PORT" "$BAUD" raw -echo -echoe -echok -crtscts cs8 -parenb -cstopb
  exec cat "$PORT" >"$log"
fi

command -v picocom >/dev/null 2>&1 || { echo "[error] picocom missing — scripts/install-deps.sh" >&2; exit 1; }
echo "[board] interactive (quit: C-a C-x); the log keeps everything the board prints"
# --imap lfcrlf keeps bare-LF firmware output readable; the log file stays raw.
exec picocom --baud "$BAUD" --imap lfcrlf --logfile "$log" "$PORT"
