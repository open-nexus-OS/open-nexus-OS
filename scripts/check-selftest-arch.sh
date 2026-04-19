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
#   3. Marker strings ("SELFTEST: …", "dsoftbusd: …", "dsoftbus: …") live
#      ONLY in `markers.rs` + `markers_generated.rs` (the manifest SSOT;
#      P4-04 emptied the [marker_emission] allowlist when emit sites
#      migrated to `crate::markers::M_<KEY>` constants generated from
#      `proof-manifest.toml`). Any literal anywhere else is a hard fail.
#   4. mod.rs files contain no `fn` definitions outside re-exports, with
#      [mod_rs_fn] allowlist for the documented OS entry-point pattern.
#   5. No .rs file >= 500 LoC outside [size_500] allowlist.
#   6. (TASK-0023B P4-10) No `REQUIRE_*` env literal hard-coded inside any
#      `test-*` or `ci-*` recipe body in the workspace `justfile`. All
#      `REQUIRE_*` env wiring must flow through
#      `nexus-proof-manifest list-env --profile=<name>` (i.e. live in
#      `proof-manifest.toml`). Allowlist `[justfile_require_env]` exists
#      for documented escapes (e.g. `RUN_OS2VM=1` is not REQUIRE_* and
#      thus never matches).
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

# ---- Rule 3: marker strings only in markers.rs + markers_generated.rs -------
# P4-04 onward: `proof-manifest.toml` is the SSOT; `build.rs` generates
# `markers_generated.rs`; emit sites reference `crate::markers::M_<KEY>`.
# No allowlist — any literal anywhere else fails the gate.
echo "==> Rule 3: marker strings outside markers.rs / markers_generated.rs"
candidates=$(rg -l '"(SELFTEST: |dsoftbusd: |dsoftbus: )' "$SC" 2>/dev/null || true)
new_violators=""
while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    rel="${f#$SC/}"
    case "$rel" in
        markers.rs)            continue ;;
        markers_generated.rs)  continue ;;
    esac
    # Filter out files where every hit is a code comment (`//`).
    real_hits=$(rg -n '"(SELFTEST: |dsoftbusd: |dsoftbus: )' "$f" \
        | rg -v '^[^:]*:[0-9]+:[[:space:]]*//' || true)
    [[ -z "$real_hits" ]] && continue
    new_violators+="${rel}"$'\n'
done <<< "$candidates"
if [[ -n "$new_violators" ]]; then
    fail "marker literals outside markers.rs / markers_generated.rs (use crate::markers::M_<KEY>):"
    printf '    %s\n' $new_violators
else
    echo "    none (manifest SSOT clean)"
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

# ---- Rule 6: no REQUIRE_* env literal in justfile test-*/ci-* recipes ------
# TASK-0023B P4-10 closure: profile dispatch is the SSOT. A hard-coded
# REQUIRE_QEMU_DHCP_STRICT=1 (or similar) inside a `just ci-os-foo` body
# bypasses the manifest and re-introduces the dual-truth surface that
# `proof-manifest.toml` was created to eliminate.
echo "==> Rule 6: no REQUIRE_* env literal in justfile test-*/ci-* recipe bodies"
JUSTFILE="$ROOT/justfile"
require_allow=$(allowlist_section "justfile_require_env")
require_violations=""
if [[ -f "$JUSTFILE" ]]; then
    # Walk the justfile recipe-by-recipe. A recipe header matches
    # `^(test|ci)-[A-Za-z0-9_-]+(\s.*)?:`; the body is every subsequent
    # line that starts with whitespace until the next non-indented line.
    current=""
    while IFS= read -r line || [[ -n "$line" ]]; do
        if [[ "$line" =~ ^(test|ci)-[A-Za-z0-9_-]+([[:space:]].*)?:[[:space:]]*$ ]]; then
            current="${line%%:*}"
            current="${current%% *}"
            continue
        fi
        # End of recipe body: a non-indented, non-empty line that isn't a comment.
        if [[ -n "$current" && -n "$line" && ! "$line" =~ ^[[:space:]] && ! "$line" =~ ^# ]]; then
            current=""
        fi
        # Skip comment-only lines (`#` or `    # …`) — they don't execute.
        if [[ "$line" =~ ^[[:space:]]*# ]]; then
            continue
        fi
        if [[ -n "$current" && "$line" =~ REQUIRE_[A-Z0-9_]+ ]]; then
            if printf '%s\n' "$require_allow" | grep -qxF "$current"; then
                continue
            fi
            require_violations+="${current}: ${line}"$'\n'
        fi
    done < "$JUSTFILE"
fi
if [[ -n "$require_violations" ]]; then
    fail "REQUIRE_* env literals in justfile recipes (move to proof-manifest.toml or allowlist):"
    printf '    %s\n' "$require_violations"
else
    echo "    none (manifest-driven REQUIRE_* SSOT clean)"
fi

if [[ "$failed" -eq 1 ]]; then
    echo ""
    echo "[FAIL] selftest-client architecture gate"
    echo "       See ADR-0027 + RFC-0038 for rule rationale."
    echo "       Allowlists: $ALLOWLIST"
    exit 1
fi

echo ""
echo "[PASS] selftest-client architecture gate (6/6 rules clean)"
