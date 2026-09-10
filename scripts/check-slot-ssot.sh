#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Slot-SSOT ratchet (TASK-0324 P4, RFC-0093 §4). Capability slot numbers have ONE
# home — `nexus-service-topology` — read by init (which provisions them), by services (which
# compile against the same constants) and by the app-child view in `nexus-sdk-routes`. Before
# P4 the same number lived three times: a bespoke init arm, a `const … SLOT: u32 = N` in the
# consumer, and a comment describing the order; a service wired in one place and not the other
# was a boot crash, not a compile error (the 2026-09 display chain).
#
# The migration runs ONE CONSUMER PER PACKAGE (P4a-P4f), so the remaining positional
# declarations are grandfathered per file in config/slot-ssot-baseline.txt and may only
# SHRINK — exactly like the LOC ratchet. A new file, or a file that grows, fails.
# Regenerate after a real migration: scripts/check-slot-ssot.sh --update-baseline
set -euo pipefail
cd "$(dirname "$0")/.."

BASELINE=config/slot-ssot-baseline.txt
PATTERN='const [A-Z_]*SLOT[A-Z_]*: u32 = (0x)?[0-9a-fA-F]+'
SCAN=(source/services source/drivers source/apps source/init)

scan() {
  grep -rnE "$PATTERN" "${SCAN[@]}" 2>/dev/null \
    | grep -v '/generated/\|_generated.rs' \
    | cut -d: -f1 | sort | uniq -c | awk '{print $1"\t"$2}' | sort -k2
}

if [[ "${1:-}" == "--update-baseline" ]]; then
  {
    echo "# Positional capability-slot declarations still outside nexus-service-topology."
    echo "# Ratchet: a file may SHRINK, never grow; a new file is a hard failure."
    echo "# Regenerate ONLY after a real P4 consumer migration:"
    echo "#   scripts/check-slot-ssot.sh --update-baseline"
    scan
  } > "$BASELINE"
  echo "[info] slot-ssot baseline updated ($(grep -cv '^#' "$BASELINE") files)"
  exit 0
fi

fail=0
current=$(scan)
declare -A base
while read -r count file; do
  [[ "$count" == \#* || -z "${file:-}" ]] && continue
  base["$file"]=$count
done < <(grep -v '^#' "$BASELINE")

while read -r count file; do
  [[ -z "${file:-}" ]] && continue
  if [[ -z "${base[$file]:-}" ]]; then
    echo "[FAIL] slot-ssot: $file declares positional capability slots ($count) — declare them in nexus-service-topology instead" >&2
    fail=1
  elif (( count > base[$file] )); then
    echo "[FAIL] slot-ssot: $file grew ${base[$file]} -> $count positional slot declarations (baseline is a ratchet)" >&2
    fail=1
  fi
done <<< "$current"

if [[ "$fail" == "0" ]]; then
  total=$(awk -F'\t' '{s+=$1} END{print s+0}' <<< "$current")
  files=$(wc -l <<< "$current" | tr -d ' ')
  echo "[PASS] slot-ssot: $total positional slot declarations in $files files (ratchet; P4a-P4f migrate them)"
fi
exit "$fail"
