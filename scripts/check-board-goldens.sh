#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: The board trees' goldens stay the trees the boards boot (TASK-0260B). Each
#          config/board/<board>/board.dts is compiled the one way every consumer gets it
#          (scripts/build-board-dtb.sh) and compared byte for byte with its golden under
#          source/libs/nexus-fdt/tests/goldens/, which the nexus-fdt and nexus-soc host tests
#          read. A tree edited without its golden would leave the host tests proving an older
#          tree than the FIT carries to the board — the check that the pinned firmware finds
#          every hart on the CLINT included. dtc's output is deterministic, so any difference
#          is a real one.
# OWNERS:  @runtime @tools-team
# STATUS:  Functional
# API_STABILITY: Internal (a `just check` gate)
# TEST_COVERAGE: TASK-0260B (a tree edited without its golden fails; regenerated, it passes)
# DEPENDS_ON: cpp + dtc (scripts/build-board-dtb.sh)
#
# Exit codes: 0 every golden matches its tree · 1 a golden is stale, missing, or a tool is missing

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GOLDENS="$ROOT/source/libs/nexus-fdt/tests/goldens"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

bad=0
n=0
for dts in "$ROOT"/config/board/*/board.dts; do
  board="$(basename "$(dirname "$dts")")"
  golden="$GOLDENS/$board.dtb"
  n=$((n + 1))
  if [ ! -f "$golden" ]; then
    echo "[FAIL] board goldens: $board has no golden — scripts/build-board-dtb.sh $board" >&2
    bad=1
    continue
  fi
  "$ROOT/scripts/build-board-dtb.sh" "$board" "$tmp/$board.dtb" >/dev/null
  if ! cmp -s "$tmp/$board.dtb" "$golden"; then
    echo "[FAIL] board goldens: $golden is not config/board/$board/board.dts — scripts/build-board-dtb.sh $board" >&2
    bad=1
  fi
done
[ "$n" -gt 0 ] || { echo "[FAIL] board goldens: no config/board/*/board.dts found" >&2; exit 1; }
[ "$bad" = 0 ] || exit 1
echo "[PASS] board goldens: $n board tree(s) byte-equal to their goldens"
