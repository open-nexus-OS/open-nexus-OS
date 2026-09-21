#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: The permission half of the board tooling (TASK-0327): install the
#          udev rule that hands the board's USB vendor id (361c) to the seat
#          user, and put the invoking user into the distro's serial group so
#          the USB-UART adapter's /dev/ttyUSB* is usable. Packages are
#          scripts/install-deps.sh's job (BOARD list); this script is the one
#          place that touches /etc, and `make initial-setup` runs it as step
#          7/7. Idempotent: a second run changes nothing and says so.
#
#          Why a separate script: install-deps.sh authenticates sudo only when
#          packages are missing; the rule + group are needed even on a box
#          where every package is already there, and a developer who never
#          flashes a board can skip exactly this step (BOARD=0).
# OWNERS:  @tools-team
# STATUS:  Functional
# API_STABILITY: Stable
# TEST_COVERAGE: scripts/check-deps.sh ("Board tools" section is the post-condition)
# DEPENDS_ON: sudo, systemd-udev, usermod
#
# Usage:
#   scripts/install-board-access.sh          # prompt before sudo
#   scripts/install-board-access.sh --yes    # non-interactive
#   scripts/install-board-access.sh --check  # report only, exit 3 if not set up
#
# Env:
#   NEXUS_FORCE_FAMILY=debian|fedora|arch   override distro detection (testing)
#
# Exit codes:
#   0  rule installed + group membership configured (or already were)
#   1  unsupported distro
#   2  sudo unavailable / install failed
#   3  --check ran and something is missing

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RULE_SRC="$REPO_ROOT/config/udev/71-nexus-board.rules"
RULE_DST="/etc/udev/rules.d/71-nexus-board.rules"
BOARD_VID="361c"

ASSUME_YES=0
CHECK_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --yes|-y) ASSUME_YES=1 ;;
    --check)  CHECK_ONLY=1 ;;
    -h|--help) sed -n '22,33p' "$0"; exit 0 ;;
    *) echo "[error] unknown flag: $arg" >&2; exit 1 ;;
  esac
done

log()  { printf '\033[1;34m[board]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[board][warn]\033[0m %s\n' "$*" >&2; }
err()  { printf '\033[1;31m[board][error]\033[0m %s\n' "$*" >&2; }

# --------- distro family → serial group --------------------------------------
# The group that owns /dev/ttyUSB* is the distro's choice, not ours; the udev
# rule reuses it so one membership covers the flash port and the console.
family="${NEXUS_FORCE_FAMILY:-}"
if [ -z "$family" ] && [ -r /etc/os-release ]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  case "${ID:-}" in
    debian|ubuntu|linuxmint|pop|raspbian) family=debian ;;
    fedora|rhel|centos|rocky|almalinux)   family=fedora ;;
    arch|manjaro|endeavouros|cachyos)     family=arch ;;
    *)
      case " ${ID_LIKE:-} " in
        *" debian "*|*" ubuntu "*) family=debian ;;
        *" fedora "*|*" rhel "*)   family=fedora ;;
        *" arch "*)                family=arch ;;
      esac
      ;;
  esac
fi
case "$family" in
  debian|fedora) SERIAL_GROUP=dialout ;;
  arch)          SERIAL_GROUP=uucp ;;
  *) err "unsupported distro family '${family:-unknown}' — install $RULE_DST by hand"; exit 1 ;;
esac

user="$(id -un)"
rule_ok=0
group_configured=0
group_effective=0

# The installed rule must be OUR rule with the placeholder substituted.
if [ -r "$RULE_DST" ] && grep -q "idVendor}==\"$BOARD_VID\"" "$RULE_DST" \
   && grep -q "GROUP=\"$SERIAL_GROUP\"" "$RULE_DST" \
   && ! grep -q "__NEXUS_SERIAL_GROUP__" "$RULE_DST"; then
  rule_ok=1
fi
if getent group "$SERIAL_GROUP" | awk -F: '{print $4}' | tr ',' '\n' | grep -qx "$user"; then
  group_configured=1
fi
if id -nG "$user" | tr ' ' '\n' | grep -qx "$SERIAL_GROUP"; then
  group_effective=1
fi

if [ "$CHECK_ONLY" = 1 ]; then
  rc=0
  if [ "$rule_ok" = 1 ]; then log "udev rule present: $RULE_DST (vendor $BOARD_VID, group $SERIAL_GROUP)"; else warn "udev rule missing or stale: $RULE_DST"; rc=3; fi
  if [ "$group_effective" = 1 ]; then log "$user is in group $SERIAL_GROUP (effective)"
  elif [ "$group_configured" = 1 ]; then warn "$user is in group $SERIAL_GROUP but this session predates it — log out and in"; rc=3
  else warn "$user is not in group $SERIAL_GROUP"; rc=3; fi
  exit "$rc"
fi

if [ "$rule_ok" = 1 ] && [ "$group_configured" = 1 ]; then
  log "board access already set up (rule + group $SERIAL_GROUP); nothing to do."
  [ "$group_effective" = 1 ] || warn "group membership is not effective in this session yet — log out and in."
  exit 0
fi

# --------- sudo, authenticated once up front (same shape as install-deps.sh) --
if [ "$(id -u)" -ne 0 ]; then
  if [ "$ASSUME_YES" != 1 ] && [ -t 0 ]; then
    printf '[board] About to install %s and add %s to group %s with sudo. Continue? [y/N] ' "$RULE_DST" "$user" "$SERIAL_GROUP"
    read -r reply
    case "$reply" in y|Y|yes|YES) ;; *) err "aborted by user."; exit 2 ;; esac
  fi
  if ! sudo -n true 2>/dev/null; then
    if [ -t 0 ]; then
      sudo -v || { err "sudo authentication failed."; exit 2; }
    else
      err "sudo needs a password but this shell has no terminal."
      err "Run from an interactive terminal: cd $REPO_ROOT && ./scripts/install-board-access.sh --yes"
      exit 2
    fi
  fi
fi

if [ "$rule_ok" != 1 ]; then
  tmp="$(mktemp)"
  sed "s/__NEXUS_SERIAL_GROUP__/$SERIAL_GROUP/" "$RULE_SRC" >"$tmp"
  sudo install -m 0644 -o root -g root "$tmp" "$RULE_DST"
  rm -f "$tmp"
  # Reload so an already-connected board gets the new permissions without a replug.
  sudo udevadm control --reload
  sudo udevadm trigger --subsystem-match=usb --attr-match="idVendor=$BOARD_VID" 2>/dev/null || true
  log "installed $RULE_DST (vendor $BOARD_VID → group $SERIAL_GROUP + uaccess)"
fi

if [ "$group_configured" != 1 ]; then
  sudo usermod -aG "$SERIAL_GROUP" "$user"
  log "added $user to group $SERIAL_GROUP"
  warn "group membership takes effect at your next login (or 'newgrp $SERIAL_GROUP' in one shell)."
fi

log "board access set up. Verify with: make doctor  (section 'Board tools')"
