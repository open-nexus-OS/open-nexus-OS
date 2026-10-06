#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: The board proof lane in one command (TASK-0327B P4 H0c). A profile's ladder is
#          judged on the desk board the way a QEMU lane is judged: the same marker vocabulary
#          (`nexus-proof-manifest verify-uart` for surprise and forbidden lines), the same FAIL
#          gate (`config/fail-marker-allow.txt`), and a REQUIRED ladder that lives here, in the
#          harness, as `scripts/qemu-test.sh` keeps its own. The board has no serial adapter at
#          the desk, so the transcript is the boot trace on the eMMC (RFC-0107), pulled by
#          `scripts/board-log.sh` once the stock system is back; the operator's hands are the
#          reset button and the microSD, prompted step by step with the LED ladder as the wait
#          signal. Nothing here passes by timeout: a trace without the loader's banner is a FAIL,
#          a missing rung names the rung, an operator-acked marker (`board-visual: <name>`,
#          `scripts/board-ack.sh`) is asked for, never assumed.
# OWNERS:  @tools-team @runtime
# STATUS:  Functional
# API_STABILITY: Stable (exit codes, `[PASS]`/`[FAIL]` lines)
# TEST_COVERAGE: `--log` against archived traces (no board): docs/board/measurements; the desk
#                board for the live path
# DEPENDS_ON: scripts/board-image.sh, board-flash.sh, board-log.sh, board-ack.sh; adb; cargo
#
# Usage:
#   scripts/board-test.sh [--profile=board-headless|board-visible] [--no-flash]
#   scripts/board-test.sh --profile=<p> --log=<uart.log>   # judge a capture, no board steps
#
# Exit codes: 0 [PASS] · 1 [FAIL] (a rung missing, a FAIL marker, a manifest violation, no banner)
#             · 2 a tool or step failed · 3 no stock system on adb · 4 an operator step refused

set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_ROOT="${CARGO_TARGET_DIR:-$ROOT/target}"
MANIFEST="$ROOT/source/apps/selftest-client/proof-manifest/manifest.toml"
# The lane-wide allow list plus the board's own known reds (each with its tracking reference).
ALLOW="$ROOT/config/fail-marker-allow.txt"
ALLOW_BOARD="$ROOT/config/fail-marker-allow-board.txt"

PROFILE="board-headless"
LOG=""
FLASH=1
ADB_WAIT_S="${BOARD_ADB_WAIT_S:-240}"

for arg in "$@"; do
  case "$arg" in
    --profile=*) PROFILE="${arg#*=}" ;;
    --log=*) LOG="${arg#*=}" ;;
    --no-flash) FLASH=0 ;;
    -h|--help) sed -n '24,29p' "$0"; exit 0 ;;
    *) echo "[error] unknown flag: $arg" >&2; exit 2 ;;
  esac
done

log() { printf '\033[1;34m[board-test]\033[0m %s\n' "$*"; }
err() { printf '\033[1;31m[board-test][error]\033[0m %s\n' "$*" >&2; }
fail() { printf '\033[1;31m[FAIL]\033[0m %s: %s\n' "$PROFILE" "$*"; exit 1; }

# An operator step: printed, waited for on the terminal, refusable. Never a timeout that
# passes: without a terminal the lane cannot run the live path at all.
prompt() {
  [ -t 0 ] || [ -r /dev/tty ] || { err "an operator step needs a terminal: $1"; exit 4; }
  printf '\033[1;33m[board-test][operator]\033[0m %s\n  Enter when done, q to abort: ' "$1"
  local ans
  read -r ans </dev/tty || ans=q
  [ "$ans" != "q" ] || { err "aborted at: $1"; exit 4; }
}

# The REQUIRED ladder per profile — the harness owns it (the manifest only names the
# vocabulary). Every literal here must be a declared marker (mirror check below). Judged by
# PRESENCE, exactly as `scripts/qemu-test.sh` judges its `expected_sequence`: the rungs come
# from independent services on four harts, so their order differs from boot to boot (measured
# 2026-09-29: `bundlemgrd: system volume verified` lands between `blkd: ready` and `blkd: trace
# os ok` in one boot and elsewhere in the next); a missing rung is reported by its position so
# the first hole names the phase that stopped.
ladder_headless=(
  "nxboot: platform="
  "nxboot: bsb ok (slot="
  "nxboot: verify ok (slot="
  "nxboot: fdt ok ("
  "nxboot: jump slot="
  "KSELFTEST: boot handoff ok (measured)"
  "KSELFTEST: platform from fdt ok ("
  "KGATE: smp bringup ok mask="
  "KSELFTEST: spawn ok pid="
  "init: ready"
  "blkd: backend ok (kind="
  "blkd: gpt ok (parts="
  "blkd: ready"
  "blkd: trace os ok (slot="
  "bundlemgrd: system volume verified ("
  "policyd: ready"
  "stage: platform"
  "SELFTEST: soc glue display ok"
  "gpud: dc encoder ok (hpd=1"
  "gpud: dc scanout ok ("
  # Block 2 (TASK-0328 U3, RFC-0099 §6): the host node and the on-board hub up through socd,
  # the DWC3 in host mode, the xHCI as measured on the stock system (version 1.10, two root
  # ports, 64 slots, 64-byte contexts, one scratchpad, line 125), the high-speed hub with its
  # five ports and TT think time on root port 1, the desk's keyboard (3434:0123) and the
  # mouse's receiver (046d:c53f) enumerated through it, hidrawd naming both.
  "init: usb host from tree ("
  "socd: bring-up /soc/storage-bus/usb@c0a00000 ok ("
  "socd: bring-up /soc/storage-bus/phy@c0a30000 ok ("
  "socd: bring-up /soc/storage-bus/phy@c0b10000 ok ("
  "socd: bring-up /usb-hub ok ("
  "xhcid: soc glue ok (host + hub up through socd)"
  "xhcid: dwc3 host ("
  "xhcid: usb2 phy ("
  "xhcid: ss phy pll ready ("
  "xhcid: controller ok (version=1.10 ports=2 slots=64 csz=64 scratch=1 irq=125)"
  "xhcid: ready (ports=2 connected="
  "xhcid: hub (slot=1 port=1 speed=high ports=5 ttt=3)"
  "xhcid: device enumerated (vid=3434 pid=0123 class=3 speed=full"
  "xhcid: hid boot interface (vid=3434 pid=0123 role=keyboard if=0 ep=0x81 mps=8 interval=1"
  "xhcid: device enumerated (vid=046d pid=c53f class=3 speed=full"
  "xhcid: hid boot interface (vid=046d pid=c53f role=mouse if=1 ep=0x82 mps=32 interval=1"
  "hidrawd: usb hid device (vid=3434 pid=0123 role=keyboard)"
  "hidrawd: usb hid device (vid=046d pid=c53f role=mouse)"
)
# Block 1's desktop (TASK-0251 P2a step 2): windowd granted the controller's framebuffer, the
# reveal switched the controller from the splash to it, windowd heard the reveal ack. Block 2's
# gate (TASK-0253B P4): a real USB HID event inside the harness's bounded wait, and the
# operator's ack that keyboard and mouse move the desktop.
ladder_visible=(
  "${ladder_headless[@]}"
  "gpud: framebuffer granted ("
  "windowd: display mode from gpud ("
  "gpud: dc reveal flip ok ("
  "windowd: desktop revealed"
  # TASK-0251 P2a step 3c: the pointer is the controller's layer — armed and read back, and
  # windowd on the overlay path (moves are `OP_MOVE_CURSOR`, no present).
  "gpud: dc cursor layer ok ("
  "windowd: hw cursor on"
  "inputd: live pointer route on"
  "inputd: live keyboard route on"
)
acks_headless=()
# TASK-0074: every visual task ships an operator rung — the shell's first modal (power button
# → alert → ESC/Cancel → Confirm → system toast) is `modal`.
acks_visible=("desktop" "typed" "pointer" "modal")

case "$PROFILE" in
  board-headless) ladder=("${ladder_headless[@]}"); acks=("${acks_headless[@]}") ;;
  board-visible) ladder=("${ladder_visible[@]}"); acks=("${acks_visible[@]}") ;;
  *) err "unknown board profile: $PROFILE (board-headless|board-visible)"; exit 2 ;;
esac

# The host `nx`/`nexus-proof-manifest`: the OS RUSTFLAGS must not leak in.
pm_cli() {
  (cd "$ROOT" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS cargo build -q --release -p nexus-proof-manifest) || return 1
  echo "$TARGET_ROOT/release/nexus-proof-manifest"
}

# ---- the live path: flash, boot, pull ------------------------------------------------------
if [ -z "$LOG" ]; then
  if [ "$FLASH" = 1 ]; then
    log "building the board image"
    "$ROOT/scripts/board-image.sh" >/dev/null || { err "board-image failed"; exit 2; }
    prompt "hold the download key (FDL) while resetting the board — download mode"
    log "flashing the plan"
    "$ROOT/scripts/board-flash.sh" --plan "$ROOT/build/board/bpi-f3/flash" --yes || { err "board-flash failed"; exit 2; }
    log "the board reboots into the eMMC (fastboot reboot)"
  else
    prompt "microSD out, reset: the board boots the eMMC"
  fi
  prompt "watch the user LED: it lights while the boot comes up and goes dark when the kernel's runtime starts (lit for good = stopped early; flicker = panic; the slow milestone ladder only with the deprecated /chosen/nexus,boot-led-ladder); when the desktop shows and a minute has passed, put the microSD in and reset — Enter when the stock system's green heartbeat shows"
  log "waiting for the stock system on adb (up to ${ADB_WAIT_S}s)"
  waited=0
  until [ "$(timeout 5 adb get-state 2>/dev/null)" = "device" ]; do
    sleep 5; waited=$((waited + 5))
    [ "$waited" -lt "$ADB_WAIT_S" ] || { err "no stock system on adb after ${ADB_WAIT_S}s"; exit 3; }
  done
  "$ROOT/scripts/board-log.sh" >/dev/null || { err "board-log failed (rc=$?)"; exit 2; }
  LOG="$ROOT/build/logs/latest-board/uart.log"
fi
[ -f "$LOG" ] || { err "no transcript at $LOG"; exit 2; }
log "judging $LOG as profile $PROFILE"

# ---- 1. the loader's banner: no banner, no boot ---------------------------------------------
grep -q "nxboot: platform=" "$LOG" || fail "no loader banner in the trace — the chain stopped before nxboot"

# ---- 2. the manifest's mirror check + surprise/forbidden lines --------------------------------
cli="$(pm_cli)" || { err "building nexus-proof-manifest failed"; exit 2; }
declared="$("$cli" list-markers --profile="$PROFILE" --manifest="$MANIFEST" --format=lines)"
for m in "${ladder[@]}"; do
  grep -qF -- "$m" <<<"$declared" || fail "ladder literal not declared in the manifest for $PROFILE: '$m'"
done
set +e
verify_out="$("$cli" verify-uart --profile="$PROFILE" --manifest="$MANIFEST" --uart="$LOG" 2>&1)"
verify_rc=$?
set -e
if [ "$verify_rc" != 0 ]; then
  printf '%s\n' "$verify_out" >&2
  fail "verify-uart reported violations"
fi

# ---- 3. the FAIL gate, as the QEMU harness keeps it ------------------------------------------
fail_lines="$(grep -aE "^(K?SELFTEST): .* FAIL|^gpud: FAIL|^xhcid: FAIL" "$LOG" || true)"
for allow in "$ALLOW" "$ALLOW_BOARD"; do
  if [ -n "$fail_lines" ] && [ -f "$allow" ]; then
    fail_lines="$(printf '%s\n' "$fail_lines" | grep -vFf <(grep -v '^#' "$allow" | sed '/^[[:space:]]*$/d') || true)"
  fi
done
if [ -n "$fail_lines" ]; then
  printf '%s\n' "$fail_lines" | sort | uniq -c >&2
  fail "FAIL markers in the trace (not allow-listed in config/fail-marker-allow.txt or config/fail-marker-allow-board.txt)"
fi
tolerated="$(grep -aE "^(K?SELFTEST): .* FAIL" "$LOG" | grep -Ff <(grep -v '^#' "$ALLOW_BOARD" 2>/dev/null | sed '/^[[:space:]]*$/d') || true)"
[ -z "$tolerated" ] || log "known board reds tolerated (config/fail-marker-allow-board.txt): $(printf '%s\n' "$tolerated" | wc -l)"

# ---- 4. the required ladder (presence, as the QEMU harness judges it) --------------------------
pos=0
prev="(start)"
for m in "${ladder[@]}"; do
  pos=$((pos + 1))
  grep -aFq -- "$m" "$LOG" || fail "rung missing: '$m' (rung $pos of ${#ladder[@]}, last present: '$prev')"
  prev=$m
done
log "ladder complete: ${#ladder[@]} rungs present"

# ---- 5. operator-acked markers (board-visible) ---------------------------------------------------
for name in "${acks[@]}"; do
  if grep -q "^board-visual: $name\$" "$LOG"; then
    continue
  fi
  if [ "$LOG" != "$ROOT/build/logs/latest-board/uart.log" ] && [ "$LOG" != "$(readlink -f "$ROOT/build/logs/latest-board/uart.log" 2>/dev/null)" ]; then
    fail "operator marker missing in the capture: board-visual: $name"
  fi
  prompt "look at the monitor: is the desktop of THIS boot showing (board-visual: $name)? Enter = yes, q = no"
  "$ROOT/scripts/board-ack.sh" "$name" >/dev/null || fail "board-ack refused '$name'"
done

printf '\033[1;32m[PASS]\033[0m %s: %d rungs, %d operator marker(s), no FAIL marker, manifest clean (%s)\n' \
  "$PROFILE" "${#ladder[@]}" "${#acks[@]}" "$LOG"
