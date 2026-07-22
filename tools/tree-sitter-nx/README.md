<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<!--
CONTEXT: Tree-sitter grammar for the `.nx` DSL — structural syntax for editors.
OWNERS: @tools-team
STATUS: Functional
API_STABILITY: Unstable
TEST_COVERAGE: `./verify.sh` — corpus tests + every tracked `.nx` file parses
ADR: n/a (editor tooling, no runtime or ABI surface)
-->

# tree-sitter-nx

A [tree-sitter](https://tree-sitter.github.io/) grammar for the `.nx` DSL.
It gives editors a real syntax **tree** instead of regex guesses: structural
highlighting, folding, indentation and text objects.

## Why it lives here and not in its own repo

`docs/dev/dsl/grammar.md` states that divergence between the documented grammar
and the compiler is a documentation bug to be fixed in the same change. The same
applies to this grammar. Keeping it in-tree means a lexer change, the grammar
change and the corpus proof land in **one** commit — a separate repository would
silently drift instead.

It is listed in the root `Cargo.toml` `exclude` array, because `members`
contains `tools/*` and a `tools/` subdirectory without a `Cargo.toml` breaks
workspace resolution (same reason `tools/qemu` is excluded).

## Source of truth

The grammar is derived from the **compiler**, not from the prose:

| What | Where |
|---|---|
| Token set, literals, escapes, sigils | `userspace/dsl/core/src/lexer.rs` |
| Declarations, views, statements, expressions | `userspace/dsl/core/src/parser/*.rs` |
| Normative prose (EBNF) | `docs/dev/dsl/grammar.md` |

Where prose and compiler disagreed, the compiler won. Deriving this grammar surfaced
about twenty divergences in `grammar.md` — behaviour the parser already had, that the
prose either omitted or described wrongly. The largest:

- `Window { style:, mode:, level:, resizable: }` declarations were absent entirely
- `Component` may carry a `state: { ... }` block after `props: { ... }`
- handler action `navigate(expr)` alongside `dispatch` / `emit`, and `emit` takes an
  expression rather than an identifier
- handlers may appear *inside* a widget's brace body, not only after it
- a widget's positional sugar and prop block are each optional (`Spacer` is complete)
- the statement `;` belongs to the in-block form, not to a single-statement arm
- `Args` may mix positional and named (`svc.settings.set("k", v, timeoutMs: 250)`)
- a plain dotted path (`user.name`) was not expressible by the old `CallChain`

**All fixed** in `grammar.md` v1.1 (2026-07-22); its changelog carries the full list.
No language change was involved — only the prose was wrong.

## Build and install

```bash
sudo pacman -S tree-sitter-cli    # one-time
./install.sh                      # generate + build + link into Neovim
```

`install.sh` symlinks rather than copies:

```
~/.config/nvim/parser/nx.so  ->  tools/tree-sitter-nx/nx.so
~/.config/nvim/queries/nx    ->  tools/tree-sitter-nx/queries
```

So editing `queries/highlights.scm` takes effect on the next file open, with no
sync step and no second copy to drift. Re-run `install.sh` only after changing
`grammar.js`.

Neovim ≥ 0.11 loads parsers from `runtimepath/parser/` natively, so `nx` needs
no `nvim-treesitter` registration — that plugin only manages the stock
languages.

## Verify

```bash
./verify.sh
```

Two hard gates:

1. **Corpus tests** (`tree-sitter test`) — one case per language construct in
   `test/corpus/`.
2. **Every tracked `.nx` file in the repo parses with zero ERROR nodes.**

Gate 2 is the one that counts. A grammar that highlights three hand-picked
examples but produces ERROR nodes on real pages is not finished — it only looks
finished. Any ERROR is a real failure.

## Layout

```
grammar.js              the grammar (derived from lexer.rs + parser/*.rs)
tree-sitter.json        CLI metadata: name, scope, file types, query paths
queries/highlights.scm  capture classes; mirrors the VS Code extension's scopes
queries/folds.scm       foldable regions
queries/indents.scm     indent hints (Helix / nvim-treesitter indent module)
test/corpus/*.txt       corpus tests
src/parser.c            generated, committed — consumers need no CLI
install.sh              generate + build + link into Neovim
verify.sh               the two gates above
```

## Two things the grammar deliberately does not do

**It does not validate.** `nx-dsl lint` stays the authority on what is a legal
program. The grammar accepts a slightly wider language so that a file being
edited stays highlighted while it is briefly incomplete.

**It does not decide formatting.** `nx-dsl fmt` defines the one canonical layout
(`parse → fmt → parse` is idempotent). `queries/indents.scm` only makes typing
feel right; when in doubt, run the formatter.

## Notable grammar decisions

- **`type_identifier` is an `alias`, not a token.** The DSL has exactly one
  identifier token — capitalisation is a convention the compiler never checks.
  A second token with the same pattern would be a lexical conflict wherever both
  are valid, e.g. inside a widget body, where `foo:` is a property and `Foo {`
  is a child widget.
- **One `widget_body` rule covers props, children and collection templates.**
  `{ row in … }` and `{ row: expr }` diverge only on the token *after* the
  identifier, so a single merged rule resolves with one lookahead; two competing
  rules would need a declared LR conflict.
- **`widget` is `prec.right`.** A trailing `on Tap -> …` inside a parent body
  could attach to the widget just parsed or to the enclosing body. `parser/view.rs`
  resolves it greedily in favour of the inner widget; `prec.right` makes the LR
  parser shift rather than reduce, reproducing exactly that.
- **Contextual keywords come for free** from `word: $ => $.identifier`.
  Tree-sitter lexes a whole word and only treats it as a keyword if that keyword
  is valid in the current parse state, so `Query`, `params`, `where`, `orderBy`,
  `limit`, `asc` and `desc` stay usable as ordinary names.

## Editor mapping

Capture names follow the Neovim standard groups and are chosen to match the
scopes the VS Code extension (`nx-dsl-vscode`) already assigns, so both editors
agree:

| TextMate scope (VS Code ext) | Tree-sitter capture |
|---|---|
| `keyword.other.declaration.nx` | `@keyword` |
| `variable.language.state.nx` | `@variable.builtin` |
| `keyword.other.effect.nx` (`@effect`) | `@attribute` |
| `support.function.i18n.nx` (`@t`) | `@function.builtin` |
| `entity.name.function.modifier.nx` | `@function.method.call` |
| `entity.name.type.nx` | `@type` |
| `constant.numeric.fixed.nx` | `@number.float` |
