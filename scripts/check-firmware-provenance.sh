#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: The firmware provenance gate (TASK-0329 P0, CLAUDE.md "Firmware blobs only under
#   resources/firmware/<device>/ with provenance + licence and a gate"): every binary under
#   resources/firmware/ is named in its directory's README.md with its sha256, the hash holds,
#   and a LICENSE sits beside it. A blob nobody documented, a patched blob or a blob without its
#   licence fails `just check`.
# OWNERS: @runtime
# STATUS: Functional
# TEST_COVERAGE: `just firmware-provenance` (in `just check`)
# USAGE: scripts/check-firmware-provenance.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FW_ROOT="$ROOT/resources/firmware"

fail() { printf '\033[1;31m[FAIL]\033[0m firmware-provenance: %s\n' "$*" >&2; exit 1; }

[ -d "$FW_ROOT" ] || { echo "[PASS] firmware-provenance: no firmware in the tree"; exit 0; }

blobs=0
while IFS= read -r -d '' blob; do
  dir="$(dirname "$blob")"
  name="$(basename "$blob")"
  readme="$dir/README.md"
  [ -f "$readme" ] || fail "$blob has no README.md beside it"
  [ -f "$dir/LICENSE" ] || fail "$blob has no LICENSE beside it"
  grep -Fq "$name" "$readme" || fail "$readme does not name $name"
  sum="$(sha256sum "$blob" | cut -d' ' -f1)"
  grep -Fq "$sum" "$readme" || fail "$readme does not carry the sha256 of $name ($sum)"
  blobs=$((blobs + 1))
done < <(find "$FW_ROOT" -type f ! -name 'README.md' ! -name 'LICENSE' ! -name '*.md' -print0)

echo "[PASS] firmware-provenance: $blobs blob(s) named with sha256 and licence"
