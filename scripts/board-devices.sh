#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: "Is the reference board on the desk, and in which mode?" — the
#          precondition every other `just board-*` recipe checks (TASK-0327).
#          The SoC (SpaceMiT K1) shows USB vendor id 361c in every mode:
#            361c:0008  the stock system's gadget (adb) — a booted system
#            361c:1001  boot-ROM download mode, and U-Boot's fastboot mode
#          plus whatever USB-UART adapter carries the debug console
#          (/dev/serial/by-id/* — a 3.3 V TTL adapter on the 3-pin UART0 header).
# OWNERS:  @tools-team
# STATUS:  Functional
# API_STABILITY: Stable (exit codes are the contract)
# TEST_COVERAGE: scripts/check-deps.sh prints the same two lines; used by board-flash/board-serial
# DEPENDS_ON: usbutils (lsusb)
#
# Usage:
#   scripts/board-devices.sh            # human report
#   scripts/board-devices.sh --mode     # print one word: stock | download | other | none
#   scripts/board-devices.sh --serial   # print the console port path (or nothing)
#
# Exit codes:
#   0  a board is connected (any mode)
#   3  no board on the USB bus (a serial adapter alone does not count)

set -euo pipefail

BOARD_VID="361c"
PID_STOCK="0008"
PID_DOWNLOAD="1001"

usage() { sed -n '18,26p' "$0"; }
MODE_ONLY=0
SERIAL_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --mode)   MODE_ONLY=1 ;;
    --serial) SERIAL_ONLY=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[error] unknown flag: $arg" >&2; exit 1 ;;
  esac
done

# Prefer a stable by-id path (survives replugs); fall back to the first ttyUSB/ttyACM.
serial_port() {
  local p
  for p in /dev/serial/by-id/*; do
    [ -e "$p" ] && { printf '%s\n' "$p"; return 0; }
  done
  for p in /dev/ttyUSB0 /dev/ttyUSB1 /dev/ttyACM0 /dev/ttyACM1; do
    [ -e "$p" ] && { printf '%s\n' "$p"; return 0; }
  done
  return 1
}

if [ "$SERIAL_ONLY" = 1 ]; then
  serial_port || true
  exit 0
fi

command -v lsusb >/dev/null 2>&1 || { echo "[error] lsusb missing (usbutils) — scripts/install-deps.sh" >&2; exit 1; }
line="$(lsusb 2>/dev/null | grep -i " ID ${BOARD_VID}:" | head -1 || true)"

mode="none"
if [ -n "$line" ]; then
  case "$line" in
    *"${BOARD_VID}:${PID_DOWNLOAD}"*) mode="download" ;;
    *"${BOARD_VID}:${PID_STOCK}"*)    mode="stock" ;;
    *)                                mode="other" ;;
  esac
fi

if [ "$MODE_ONLY" = 1 ]; then
  printf '%s\n' "$mode"
  [ "$mode" != "none" ] && exit 0
  exit 3
fi

case "$mode" in
  download) echo "[board] download/fastboot mode: $line"
            echo "        → just board-flash can talk to it now" ;;
  stock)    echo "[board] stock system running (adb gadget): $line"
            echo "        → for flashing: hold the download key (FDL) while resetting, or 'fastboot usb 0' at the U-Boot prompt" ;;
  other)    echo "[board] vendor $BOARD_VID in an unknown mode: $line" ;;
  none)     echo "[board] no board on the USB bus (vendor $BOARD_VID not found)" ;;
esac

if port="$(serial_port)"; then
  echo "[board] serial adapter: $port  (just board-serial)"
else
  echo "[board] no USB-UART adapter (/dev/serial/by-id empty) — the console needs a 3.3 V TTL adapter on UART0 (GND/RX/TX, 115200 8N1)"
fi

[ "$mode" != "none" ] && exit 0
exit 3
