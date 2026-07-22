#!/usr/bin/env bash
# Proof that this grammar actually covers the .nx dialect the repo is written in.
#
# Two gates, both hard:
#
#   1. `tree-sitter test` — the hand-written corpus in test/corpus/, one case
#      per language construct.
#   2. Every .nx file tracked in the repo parses with ZERO error nodes.
#
# Gate 2 is the one that matters. A grammar that highlights three hand-picked
# examples and produces ERROR nodes on real pages is not done — it just looks
# done. Any ERROR here is a real failure, not a cosmetic one.
set -uo pipefail

cd "$(dirname "$0")"
GRAMMAR_DIR="$PWD"
REPO_ROOT="$(git rev-parse --show-toplevel)"

if ! command -v tree-sitter >/dev/null 2>&1; then
    echo "error: tree-sitter CLI not found (sudo pacman -S tree-sitter-cli)" >&2
    exit 1
fi

echo "==> gate 1: corpus tests"
if ! tree-sitter test; then
    echo "FAIL: corpus tests" >&2
    exit 1
fi

echo
echo "==> gate 2: every .nx file in the repo parses without ERROR nodes"

# `find`, not `git ls-files`: apps under active development are untracked for a
# while (ime-ui was, when this was written) and those are exactly the files a
# grammar regression would hit first.
#
# NOTE: tree-sitter resolves a grammar for `.nx` from the tree-sitter.json in
# the CURRENT directory, so this must run with the grammar dir as cwd — hence
# the `cd` at the top of this script and the absolute paths below.
mapfile -t FILES < <(
    find "$REPO_ROOT" -name '*.nx' \
        -not -path "$REPO_ROOT/target*" \
        -not -path "$REPO_ROOT/build/*" | sort
)

total=0
failed=0
for file in "${FILES[@]}"; do
    total=$((total + 1))
    if ! output=$(tree-sitter parse --quiet "$file" 2>&1); then
        failed=$((failed + 1))
        echo "  ERROR ${file#"$REPO_ROOT"/}"
        # First offending node, to make the failure actionable.
        echo "$output" | grep -m1 -E '\(ERROR|MISSING' | sed 's/^/        /'
    fi
done

echo
if [ "$failed" -eq 0 ]; then
    echo "PASS: $total/$total .nx files in the repo parse cleanly"
    exit 0
fi

echo "FAIL: $failed of $total .nx files have parse errors" >&2
exit 1
