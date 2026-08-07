#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Single source of truth for the DECLARED build inputs (git submodules
#          + pinned font downloads). Both spurs call this: the `just` spur via
#          `just inputs`, the `make` spur via `make initial-setup`. Keeping the
#          list in one place is what stops the two spurs from drifting — a build
#          whose inputs are implicit measures the MACHINE, not the code.
# OWNERS:  @tools-team
# STATUS:  Functional
# API_STABILITY: Stable
# TEST_COVERAGE: Covered indirectly (every `just inputs` consumer)
# ADR: docs/architecture/02-selftest-and-ci.md
#
# Usage:
#   scripts/fetch-inputs.sh                  # submodules + fonts
#   scripts/fetch-inputs.sh --submodules     # submodules only
#   scripts/fetch-inputs.sh --fonts          # pinned fonts only
#   scripts/fetch-inputs.sh --check          # report only, exit 3 if incomplete
#   scripts/fetch-inputs.sh --list-submodules  # print the paths, one per line
#
# Exit codes:
#   0  inputs present (or fetched successfully)
#   1  bad usage / fetch failed
#   3  --check ran and inputs are missing

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# Submodules the BUILD needs. Deliberately NOT `--recursive`: tools/qemu-src is
# a 1.9 GB vendored QEMU tree that no build or test tool references (every
# script takes qemu-system-riscv64 from PATH). Recursing into it also pulls its
# nested roms/edk2 + OpenSSL, whose vendored sources redden structure-gate.
BUILD_SUBMODULES=(
  resources/fonts/inter
  resources/icons/lucide
  resources/cursors/mocu
)

# One representative file per submodule — `git submodule status` reports a
# leading '-' for uninitialized, but a checked-out-yet-empty directory would
# still fool it, and these are the exact paths the build scripts include.
declare -A SUBMODULE_PROBE=(
  [resources/fonts/inter]="resources/fonts/inter/docs/font-files/InterVariable.ttf"
  [resources/icons/lucide]="resources/icons/lucide/icons/wallet.svg"
  [resources/cursors/mocu]="resources/cursors/mocu/src/svg/default.svg"
)

DO_SUBMODULES=1
DO_FONTS=1
CHECK_ONLY=0

for arg in "$@"; do
  case "$arg" in
    --submodules) DO_FONTS=0 ;;
    --fonts)      DO_SUBMODULES=0 ;;
    --check)      CHECK_ONLY=1 ;;
    --list-submodules)
      printf '%s\n' "${BUILD_SUBMODULES[@]}"
      exit 0
      ;;
    -h|--help)    sed -n '19,30p' "$0"; exit 0 ;;
    *) echo "[error] unknown flag: $arg" >&2; exit 1 ;;
  esac
done

log()  { printf '\033[1;34m[inputs]\033[0m %s\n' "$*"; }
err()  { printf '\033[1;31m[inputs][error]\033[0m %s\n' "$*" >&2; }

missing=0

if [ "$DO_SUBMODULES" = 1 ]; then
  need=()
  for sm in "${BUILD_SUBMODULES[@]}"; do
    probe="${SUBMODULE_PROBE[$sm]}"
    if [ -e "$probe" ]; then
      continue
    fi
    need+=("$sm")
  done

  if [ "${#need[@]}" -eq 0 ]; then
    log "submodules present (${#BUILD_SUBMODULES[@]} checked)"
  elif [ "$CHECK_ONLY" = 1 ]; then
    err "submodules not initialized: ${need[*]}"
    err "fix: scripts/fetch-inputs.sh --submodules"
    missing=1
  else
    log "initializing submodules: ${need[*]}"
    git submodule update --init "${need[@]}"
  fi
fi

if [ "$DO_FONTS" = 1 ]; then
  # Pinned Noto Sans CJK OTFs: ~50 MB, gitignored, fetched at a pinned commit +
  # SHA-256. fetch-fonts.sh is a no-op once they verify, so in non-check mode we
  # simply delegate — it IS the check.
  if [ "$CHECK_ONLY" = 1 ]; then
    if compgen -G "resources/fonts/noto/*.otf" >/dev/null; then
      log "pinned Noto fonts present"
    else
      err "pinned Noto Sans CJK fonts missing (resources/fonts/noto/)"
      err "fix: scripts/fetch-inputs.sh --fonts"
      missing=1
    fi
  else
    scripts/fetch-fonts.sh
  fi
fi

if [ "$CHECK_ONLY" = 1 ]; then
  [ "$missing" -eq 0 ] || exit 3
  log "all declared build inputs present."
  exit 0
fi

log "build inputs ready."
