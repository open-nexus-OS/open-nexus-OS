#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Postflight ladder for the SystemUI DSL bootstrap track
# (TASK-0080C DoD): greps the newest boot UART log for the marker chain in
# stage order. Stages whose OS wiring has not landed yet report SKIP (with
# the gating task) instead of failing — the script never fakes a pass: a
# stage is OK only if its markers are actually present.
#
# Usage: tools/postflight-systemui-bootstrap-shell.sh [uart.log]
#   Without an argument the newest build/logs/*/uart.log is used.
#   Exit 0 = no FAIL stages (SKIPs allowed), exit 1 = at least one FAIL.

set -u

log="${1:-}"
if [[ -z "$log" ]]; then
  log="$(ls -t build/logs/*/uart.log 2>/dev/null | head -1 || true)"
fi
if [[ -z "$log" || ! -f "$log" ]]; then
  echo "postflight: no uart.log found (run a boot first)" >&2
  exit 1
fi
echo "postflight: $log"

fails=0

# check <stage> <required marker...> — OK iff every marker is present.
check() {
  local stage="$1"
  shift
  local missing=()
  for marker in "$@"; do
    grep -qF -- "$marker" "$log" || missing+=("$marker")
  done
  if [[ ${#missing[@]} -eq 0 ]]; then
    echo "  OK    $stage"
  else
    echo "  FAIL  $stage — missing: ${missing[*]}"
    fails=$((fails + 1))
  fi
}

# check_any <stage> <marker>... — OK iff at least ONE marker is present
# (interactive boots FOLD routine service markers into `OK <svc> n/n`
# verdict lines; headless full-marker boots print the raw form).
check_any() {
  local stage="$1"
  shift
  for marker in "$@"; do
    if grep -qE -- "$marker" "$log"; then
      echo "  OK    $stage"
      return
    fi
  done
  echo "  FAIL  $stage — none of: $*"
  fails=$((fails + 1))
}

# interactive <stage> <marker...> — markers that only appear after a live
# user interaction (click lane): OK when present, PENDING (not a failure)
# when the boot had no interaction.
interactive() {
  local stage="$1"
  shift
  local missing=()
  for marker in "$@"; do
    grep -qF -- "$marker" "$log" || missing+=("$marker")
  done
  if [[ ${#missing[@]} -eq 0 ]]; then
    echo "  OK    $stage"
  else
    echo "  PEND  $stage — needs a live click; missing: ${missing[*]}"
  fi
}

skip() {
  echo "  SKIP  $1 — $2"
}

echo "== boot base =="
check "init launch routes" \
  "init: windowd route->abilitymgr ok" \
  "init: abilitymgr route->execd ok"
check "registry + caps" \
  "abilitymgr: registry ok (n=" \
  "abilitymgr: caps ok app=counter"
check_any "sessiond up" \
  "sessiond: ready" \
  "OK +sessiond"
check_any "greeter/shell surface" \
  "windowd: greeter visible" \
  "(OK|WARN) +windowd"
check "DSL in-compositor mount (mount-only since demo retirement)" \
  "DSL: program loaded hash="

echo "== launch e2e (RFC-0065 + ADR-0042 + GET_PAYLOAD) =="
interactive "launch chain" \
  "abilitymgr: launch (app=" \
  "abilitymgr: spawn ok" \
  "execd: apphost windowd route granted"
interactive "payload chain (GET_PAYLOAD)" \
  "execd: app payload requested" \
  "bundlemgrd: payload served" \
  "execd: app payload granted" \
  "APPHOST: payload source=bundle"
interactive "app surface" \
  "APPHOST: mounted hash=" \
  "WINDOWD: surface created" \
  "WINDOWD: surface presented"
interactive "app event channel (dedicated)" \
  "execd: app event channel sent" \
  "execd: app event channel granted" \
  "WINDOWD: app event channel attached" \
  "APPHOST: events source=dedicated"
interactive "app input" \
  "WINDOWD: surface input routed" \
  "APPHOST: interactive frame presented"

echo "== DSL shell/greeter (0080C wiring pending) =="
skip "DSL greeter visible" "mount swap lands with TASK-0080C step 2"
check "DSL shell mounted (0080C step 1)" \
  "systemui: dsl shell on"
skip "queryd: ready" "os-lite queryd + idl-runtime no_std land with TASK-0080C step 4"

echo
if [[ $fails -gt 0 ]]; then
  echo "postflight: $fails stage(s) FAILED"
  exit 1
fi
echo "postflight: base stages green (PEND = needs live interaction, SKIP = wiring pending)"

# --visual (optional, pass as $2 or with a running VNC display): grab the live
# frame and verify the display is NOT black — markers are compositor claims,
# this is the display-side truth (fake-proof guard).
if [[ "${2:-}" == "--visual" || "${POSTFLIGHT_VISUAL:-0}" == "1" ]]; then
  echo "== display truth (VNC) =="
  if python3 "$(dirname "$0")/visual-postflight.py" \
    --out "${TMPDIR:-/tmp}/postflight-frame.png"; then
    echo "  OK    display non-black"
  else
    rc=$?
    if [[ $rc -eq 1 ]]; then
      echo "  FAIL  display black while markers green (silent scanout class)"
      exit 1
    fi
    echo "  SKIP  no VNC display reachable (run under just start-vnc)"
  fi
fi
