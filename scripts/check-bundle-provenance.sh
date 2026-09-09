#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Build-truth gate (TASK-0324 P0). Every system-volume bundle under
# build/system-bundles/<svc>/ must carry, in meta/launch.json, exactly the
# feature set `scripts/discover-services.sh` resolves for it right now, and the
# ELF must really contain those features: gpud prints its compiled feature set
# as its first UART line (`gpud: features=…`, markers.rs), so the string is
# grep-able in the binary. Run at the end of scripts/build.sh and by
# `just build-os-workspace`. The 2026-09-09 black screen was a gpud bundle
# built without `virgl` on a GL device — every marker green, no pixels.
set -euo pipefail
cd "$(dirname "$0")/.."
root="build/system-bundles"
[[ -d "$root" ]] || { echo "[skip] bundle-provenance: no $root"; exit 0; }
fail=0
while read -r svc; do
  dir="$root/$svc"
  [[ -d "$dir" ]] || continue
  want=$(scripts/discover-services.sh --cargo-features "$svc")
  have=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("features",""))' "$dir/meta/launch.json")
  if [[ "$have" != "$want" ]]; then
    echo "[FAIL] bundle-provenance: $svc launch.json features='$have' but the SSOT resolves '$want'" >&2
    fail=1
  fi
  if [[ "$svc" == "gpud" ]]; then
    if ! grep -aq "gpud: features=$want" "$dir/payload.elf"; then
      echo "[FAIL] bundle-provenance: gpud payload.elf does not carry 'gpud: features=$want' (built with different features?)" >&2
      fail=1
    fi
  fi
done < <(sed -e 's/#.*//' -e '/^\s*$/d' scripts/system-volume-services.txt)
if [[ "$fail" -ne 0 ]]; then exit 1; fi
echo "[PASS] bundle-provenance: every system bundle carries its resolved feature set"
