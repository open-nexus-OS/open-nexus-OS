#!/usr/bin/env bash
# CONTEXT: Structural quality gate (CLAUDE.md Conventions, .claude/skills/code-quality).
#
# Two deterministic checks, both ratcheted so legacy debt is grandfathered but
# can only shrink — new code meets the bar from day one:
#
#   1) Module size: no .rs file above LOC_LIMIT (600) unless listed in
#      config/loc-baseline.txt, and a baselined file may never GROW past its
#      recorded LOC. Shrinking is always fine; regenerate the baseline with
#      `scripts/check-structure.sh --update-loc-baseline` after real splits.
#
#   2) Service layout: every crate in config/os-services.txt (SSOT, shared with
#      dep-gate/diag-os) has src/ and a tests/ dir with at least one .rs file,
#      unless grandfathered in config/service-layout.allow.
#
# Exit 1 on any violation; silent-ish green otherwise. Scan scope: source/,
# userspace/, tools/ — excluding target/, generated code (*_generated.rs,
# */generated/*, *_capnp.rs).
set -euo pipefail

cd "$(dirname "$0")/.."
LOC_LIMIT=600
BASELINE="config/loc-baseline.txt"
LAYOUT_ALLOW="config/service-layout.allow"
SERVICES="config/os-services.txt"

scan_loc() {
    find source userspace tools -name '*.rs' \
        -not -path '*/target/*' \
        -not -name '*_generated.rs' \
        -not -path '*/generated/*' \
        -not -name '*_capnp.rs' \
        -exec wc -l {} + | awk -v lim="$LOC_LIMIT" '$2 ~ /\.rs$/ && $1 > lim {print $1 "\t" $2}'
}

if [ "${1:-}" = "--update-loc-baseline" ]; then
    {
        echo "# Grandfathered .rs files over ${LOC_LIMIT} LOC (ratchet: may shrink, never grow)."
        echo "# Regenerate ONLY after real splits: scripts/check-structure.sh --update-loc-baseline"
        scan_loc | sort -k2
    } > "$BASELINE"
    echo "structure-gate: baseline updated ($(grep -c $'\t' "$BASELINE") files)"
    exit 0
fi

fail=0

# --- 1) module size (ratchet against baseline) --------------------------------
while IFS=$'\t' read -r loc path; do
    base=$(awk -F'\t' -v p="$path" '$2 == p {print $1}' "$BASELINE")
    if [ -z "$base" ]; then
        echo "[FAIL] structure-gate: $path is $loc LOC (> $LOC_LIMIT) and not grandfathered — split by responsibility (see .claude/skills/code-quality)"
        fail=1
    elif [ "$loc" -gt "$base" ]; then
        echo "[FAIL] structure-gate: $path grew $base -> $loc LOC (baseline is a ratchet — split instead of growing)"
        fail=1
    fi
done < <(scan_loc)

# Baselined files that vanished (renamed/split) keep the baseline honest.
while IFS=$'\t' read -r _loc path; do
    [ -e "$path" ] || { echo "[FAIL] structure-gate: baseline lists missing file $path — regenerate with --update-loc-baseline"; fail=1; }
done < <(grep $'\t' "$BASELINE")

# --- 2) service layout (src/ + tests/*.rs) ------------------------------------
while read -r svc; do
    case "$svc" in ''|'#'*) continue ;; esac
    dir=""
    for cand in "source/services/$svc" "source/drivers/$svc"; do
        [ -d "$cand" ] && { dir="$cand"; break; }
    done
    if [ -z "$dir" ]; then
        echo "[FAIL] structure-gate: service '$svc' (os-services.txt) not found under source/services or source/drivers"
        fail=1; continue
    fi
    [ -d "$dir/src" ] || { echo "[FAIL] structure-gate: $dir has no src/"; fail=1; }
    if ! ls "$dir/tests"/*.rs >/dev/null 2>&1; then
        if ! grep -qx "$svc" <(grep -v '^#' "$LAYOUT_ALLOW" 2>/dev/null); then
            echo "[FAIL] structure-gate: $dir has no tests/*.rs (contract/reject tests) and is not grandfathered in $LAYOUT_ALLOW"
            fail=1
        fi
    fi
done < "$SERVICES"

if [ "$fail" -ne 0 ]; then
    echo "[FAIL] structure-gate failed"
    exit 1
fi
echo "[PASS] structure-gate: module-size ratchet + service layout ok"
