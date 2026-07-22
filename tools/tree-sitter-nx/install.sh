#!/usr/bin/env bash
# Build the .nx tree-sitter parser and wire it into Neovim.
#
# The queries are SYMLINKED rather than copied: editing
# tools/tree-sitter-nx/queries/highlights.scm then takes effect in Neovim on
# the next file open, with no sync step and no second copy to drift.
#
# Re-run this after changing grammar.js. Changing only a .scm query needs
# nothing — the symlink already points at it.
set -euo pipefail

cd "$(dirname "$0")"
GRAMMAR_DIR="$PWD"
NVIM_CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/nvim"

if ! command -v tree-sitter >/dev/null 2>&1; then
    echo "error: tree-sitter CLI not found. Install it with:" >&2
    echo "    sudo pacman -S tree-sitter-cli" >&2
    exit 1
fi

echo "==> generating parser from grammar.js"
tree-sitter generate

echo "==> building nx.so"
tree-sitter build -o nx.so

echo "==> linking into $NVIM_CONFIG"
mkdir -p "$NVIM_CONFIG/parser" "$NVIM_CONFIG/queries"
ln -sf "$GRAMMAR_DIR/nx.so" "$NVIM_CONFIG/parser/nx.so"
# -n so re-running replaces the link instead of nesting inside it.
ln -sfn "$GRAMMAR_DIR/queries" "$NVIM_CONFIG/queries/nx"

echo
echo "done:"
echo "  parser   $NVIM_CONFIG/parser/nx.so -> $GRAMMAR_DIR/nx.so"
echo "  queries  $NVIM_CONFIG/queries/nx   -> $GRAMMAR_DIR/queries"
echo
echo "Verify with:  ./verify.sh"
