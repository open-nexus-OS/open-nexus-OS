#!/usr/bin/env bash
#
# scripts/check-selftest-arch.sh
#
# TASK-0023B Cut P3-03 — mechanical architecture gate for source/apps/selftest-client.
# Enforces ADR-0027 invariants and RFC-0038 refinement (7).
#
# Rules (all relative to source/apps/selftest-client/src/):
#   1. os_lite/mod.rs <= 80 LoC
#   2. phases/*.rs MUST NOT import other phases::* modules
#   3. Marker strings ("SELFTEST: …", "dsoftbusd: …", "dsoftbus: …") only in
#      phases/* and crate::markers, with [marker_emission] allowlist for the
#      Phase-2 baseline (Phase 4 shrinks the allowlist to zero).
#   4. mod.rs files contain no `fn` definitions outside re-exports, with
#      [mod_rs_fn] allowlist for the documented OS entry-point pattern.
#   5. No .rs file >= 500 LoC outside [size_500] allowlist.
#
# Allowlists live in source/apps/selftest-client/.arch-allowlist.txt.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SC="$ROOT/source/apps/selftest-client/src"
ALLOWLIST="$ROOT/source/apps/selftest-client/.arch-allowlist.txt"

if [[ ! -d "$SC" ]]; then
    echo "[FAIL] selftest-client src dir missing: $SC" >&2
    exit 2
fi
if [[ ! -f "$ALLOWLIST" ]]; then
    echo "[FAIL] arch allowlist missing: $ALLOWLIST" >&2
    exit 2
fi

failed=0
fail() { echo "[FAIL] $*"; failed=1; }

# Read the body of a [section] from the allowlist (skip comments + blanks).
allowlist_section() {
    local section="$1"
    awk -v s="[$section]" '
        $0 == s        { in_section = 1; next }
        /^\[/          { in_section = 0 }
        in_section && NF && $0 !~ /^[[:space:]]*#/ { print }
    ' "$ALLOWLIST"
}

# ---- Rule 1: os_lite/mod.rs <= 80 LoC ---------------------------------------
echo "==> Rule 1: os_lite/mod.rs LoC ceiling (<= 80)"
loc=$(wc -l < "$SC/os_lite/mod.rs")
if [[ "$loc" -gt 80 ]]; then
    fail "os_lite/mod.rs is $loc lines (max 80)"
else
    echo "    os_lite/mod.rs: $loc LoC"
fi

# ---- Rule 2: phases/* must not import other phases::* -----------------------
echo "==> Rule 2: phases isolation (no phases::* cross-imports)"
phase_imports=$(rg -n "use [^;]*::phases::" "$SC/os_lite/phases/" 2>/dev/null || true)
if [[ -n "$phase_imports" ]]; then
    fail "phases cross-imports detected:"
    echo "$phase_imports"
else
    echo "    none"
fi

# ---- Rule 3: marker strings only in phases/* + markers.rs (allowlisted) -----
echo "==> Rule 3: marker strings outside phases/*+markers.rs (allowlist-aware)"
marker_allow=$(allowlist_section "marker_emission")
candidates=$(rg -l '"(SELFTEST: |dsoftbusd: |dsoftbus: )' "$SC" 2>/dev/null || true)
new_violators=""
while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    rel="${f#$SC/}"
    case "$rel" in
        os_lite/phases/*) continue ;;
        markers.rs)       continue ;;
    esac
    # Filter out files where every hit is a code comment (`//`).
    real_hits=$(rg -n '"(SELFTEST: |dsoftbusd: |dsoftbus: )' "$f" \
        | rg -v '^[^:]*:[0-9]+:[[:space:]]*//' || true)
    [[ -z "$real_hits" ]] && continue
    if printf '%s\n' "$marker_allow" | grep -qxF "$rel"; then
        continue
    fi
    new_violators+="${rel}"$'\n'
done <<< "$candidates"
if [[ -n "$new_violators" ]]; then
    fail "marker strings in non-allowlisted files (add to [marker_emission] or move to phases/*):"
    printf '    %s\n' $new_violators
else
    echo "    none beyond [marker_emission] baseline"
fi

# ---- Rule 4: mod.rs files contain no fn definitions (allowlisted) -----------
echo "==> Rule 4: mod.rs files contain no fn definitions (allowlist-aware)"
mod_allow=$(allowlist_section "mod_rs_fn")
mod_violations=""
while IFS= read -r -d '' f; do
    rel="${f#$SC/}"
    if printf '%s\n' "$mod_allow" | grep -qxF "$rel"; then
        continue
    fi
    hits=$(rg -n '^[[:space:]]*(pub(\(crate\))? )?fn ' "$f" 2>/dev/null || true)
    if [[ -n "$hits" ]]; then
        mod_violations+="${rel}: ${hits}"$'\n'
    fi
done < <(find "$SC" -name mod.rs -print0)
if [[ -n "$mod_violations" ]]; then
    fail "fn definitions in mod.rs (add to [mod_rs_fn] or move into a sibling file):"
    printf '    %s\n' "$mod_violations"
else
    echo "    none beyond [mod_rs_fn] baseline"
fi

# ---- Rule 5: no .rs file >= 500 LoC outside size_500 allowlist --------------
echo "==> Rule 5: file size ceiling (no >= 500 LoC outside [size_500])"
size_allow=$(allowlist_section "size_500")
size_violations=""
while IFS= read -r line; do
    loc=$(awk '{print $1}' <<< "$line")
    f=$(awk '{$1=""; sub(/^ /,""); print}' <<< "$line")
    [[ -z "$f" ]] && continue
    case "$f" in total|insgesamt) continue ;; esac
    rel="${f#$SC/}"
    if printf '%s\n' "$size_allow" | grep -qxF "$rel"; then
        continue
    fi
    size_violations+="    ${loc} ${rel}"$'\n'
done < <(find "$SC" -name '*.rs' -exec wc -l {} + | awk '$1>=500 && $2 != "total" && $2 != "insgesamt"')
if [[ -n "$size_violations" ]]; then
    fail "files >= 500 LoC outside [size_500]:"
    printf '%s' "$size_violations"
else
    echo "    none beyond [size_500] baseline"
fi

if [[ "$failed" -eq 1 ]]; then
    echo ""
    echo "[FAIL] selftest-client architecture gate"
    echo "       See ADR-0027 + RFC-0038 for rule rationale."
    echo "       Allowlists: $ALLOWLIST"
    exit 1
fi

echo ""
echo "[PASS] selftest-client architecture gate (5/5 rules clean)"
