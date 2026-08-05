#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: CI/test-all COVERAGE parity gate.
#
# `just test-all` is advertised as "green test-all => green CI". On 2026-08-05
# that promise broke in the dullest possible way: the workflow's kernel job ran
# `just lint-kernel`, `test-all` ran only `just build-kernel`, and since the
# kernel's deny(warnings) lives behind the riscv/none cfg a clean cross-build
# proved nothing about kernel clippy. Three clippy errors reached main.
#
# This gate makes that drift mechanical rather than remembered: every `just`
# recipe the workflow invokes must also be reachable from `test-all`. Deliberate
# divergences live in config/ci-parity.allow WITH a rationale, so skipping
# something is a recorded decision instead of an oversight.
#
# Scope + limits (state them, do not pretend otherwise):
#   * Only `just <recipe>` invocations are matched. CI steps that shell out
#     directly (e.g. ./scripts/qemu-test.sh) are invisible here and belong in
#     the allowlist with a note.
#   * "Reachable" is transitive through recipe dependencies, so `test-all`
#     calling `check` covers `lint`, `fmt-check`, ... automatically.
#   * This is a COVERAGE gate. It says nothing about whether the two run against
#     the same INPUTS — that is `just ci-verify`.
set -euo pipefail

cd "$(dirname "$0")/.."
WORKFLOW=".github/workflows/ci.yml"
JUSTFILE="justfile"
ALLOW="config/ci-parity.allow"

[ -f "$WORKFLOW" ] || { echo "[FAIL] ci-parity: $WORKFLOW not found"; exit 1; }

# --- 1) recipes the workflow invokes ------------------------------------------
# Matches `run: just X` and `run: |` blocks containing `just X`. Comment lines
# are stripped first — the workflow header prose ("change the just recipe and
# keep this file a dumb caller") would otherwise register as a recipe named
# `recipe`.
ci_recipes() {
    grep -v '^[[:space:]]*#' "$WORKFLOW" |
        grep -oE '(^|[^-[:alnum:]_])just[[:space:]]+[a-z][a-z0-9-]*' |
        awk '{print $NF}' | sort -u
}

# --- 2) recipes reachable from test-all ---------------------------------------
# Body lines of `test-all:` plus, transitively, the dependencies of everything
# it reaches. `just --summary` is not enough (it lists names, not edges), so
# parse the justfile: a recipe header is `name[ deps...]:` at column 0.
deps_of() {
    awk -v want="$1" '
        /^[a-z][a-z0-9-]*[^:]*:/ {
            hdr = $0; sub(/:.*/, "", hdr)
            split(hdr, parts, /[[:space:]]+/)
            name = parts[1]
            if (name == want) {
                for (i = 2; i in parts; i++) if (parts[i] != "") print parts[i]
            }
        }
    ' "$JUSTFILE"
}

body_calls_of() {
    awk -v want="$1" '
        /^[a-z][a-z0-9-]*[^:]*:/ {
            hdr = $0; sub(/:.*/, "", hdr)
            split(hdr, parts, /[[:space:]]+/)
            inrec = (parts[1] == want)
            next
        }
        inrec && /^[[:space:]]/ {
            if (match($0, /(^|[^-[:alnum:]_])just[[:space:]]+[a-z][a-z0-9-]*/)) {
                seg = substr($0, RSTART, RLENGTH)
                n = split(seg, w, /[[:space:]]+/)
                print w[n]
            }
        }
    ' "$JUSTFILE"
}

declare -A seen=()
queue=(test-all)
while [ ${#queue[@]} -gt 0 ]; do
    cur="${queue[0]}"; queue=("${queue[@]:1}")
    [ -n "${seen[$cur]:-}" ] && continue
    seen["$cur"]=1
    while read -r d; do [ -n "$d" ] && queue+=("$d"); done < <(deps_of "$cur")
    while read -r d; do [ -n "$d" ] && queue+=("$d"); done < <(body_calls_of "$cur")
done

# --- 3) compare ---------------------------------------------------------------
fail=0
while read -r recipe; do
    [ -z "$recipe" ] && continue
    [ -n "${seen[$recipe]:-}" ] && continue
    if grep -qx "$recipe" <(grep -v '^#' "$ALLOW" 2>/dev/null | grep -v '^[[:space:]]*$'); then
        continue
    fi
    echo "[FAIL] ci-parity: the workflow runs 'just $recipe' but 'just test-all' never reaches it"
    echo "       -> chain it into test-all, or record the divergence in $ALLOW with a rationale"
    fail=1
done < <(ci_recipes)

# Allowlist entries that no longer correspond to a CI step are stale.
while read -r recipe; do
    case "$recipe" in ''|'#'*) continue ;; esac
    if ! ci_recipes | grep -qx "$recipe"; then
        echo "[FAIL] ci-parity: $ALLOW lists '$recipe', which the workflow no longer runs — drop the entry"
        fail=1
    fi
done < "$ALLOW"

if [ "$fail" -ne 0 ]; then
    echo "[FAIL] ci-parity failed"
    exit 1
fi
echo "[PASS] ci-parity: every workflow recipe is reachable from test-all"
