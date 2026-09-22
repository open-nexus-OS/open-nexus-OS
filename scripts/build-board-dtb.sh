#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: compiles a board's device tree (RFC-0098 C1) the one way every consumer
# gets it: the C preprocessor resolves the binding headers under
# config/board/include (clock/reset ids, power-domain ids, the pad macro), dtc
# compiles with 512 bytes of headroom for nxboot's /chosen writes. Used for the
# nexus-fdt goldens (source/libs/nexus-fdt/tests/goldens/<board>.dtb) and, from
# TASK-0260B on, for the FIT the board boots.
#   scripts/build-board-dtb.sh <board> [out.dtb]     (default out: the golden)
set -euo pipefail
cd "$(dirname "$0")/.."
board=${1:?usage: build-board-dtb.sh <board> [out.dtb]}
src="config/board/$board/board.dts"
out=${2:-"source/libs/nexus-fdt/tests/goldens/$board.dtb"}
[ -f "$src" ] || { echo "[FAIL] no tree at $src" >&2; exit 1; }
command -v cpp >/dev/null || { echo "[FAIL] cpp missing (a C compiler's preprocessor)" >&2; exit 1; }
command -v dtc >/dev/null || { echo "[FAIL] dtc missing (make initial-setup installs it)" >&2; exit 1; }
tmp=$(mktemp --suffix=.dts)
trap 'rm -f "$tmp"' EXIT
cpp -nostdinc -I config/board/include -undef -x assembler-with-cpp -P "$src" > "$tmp"
dtc -I dts -O dtb -p 512 -o "$out" "$tmp"
echo "[ok]   $out ($(stat -c %s "$out") bytes) from $src"
