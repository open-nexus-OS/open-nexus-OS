<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Text Input

Contract: `docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md`.

## Focus model (two-level, RFC-0075)

1. **Surface focus** — windowd authority (pointer/touch-down hit-test).
2. **Widget focus** — app authority: the DSL runtime focuses a
   TextField/TextArea on tap (`View::focus_text_at`), tracks it across
   re-emits by binding identity (store + field path), and the host announces
   every transition upward via `OP_SURFACE_TEXT_FOCUS` (focused, field kind
   text/password, caret rect).

Editing ops on the focused field: `View::insert_text` (append-at-end v1 caret
model, bounded to 256 chars) and `View::backspace_text`. Committed text
arrives from windowd as `OP_SURFACE_TEXT` — apps never see raw key codes for
text entry. The legacy positional `View::text_input(x, y, text)` entry point
remains for host fixtures only.

## Password fields

`TextField { secure: true }` renders bullets (the real value never enters the
scene tree), reports `secure` in the focus snapshot, and downstream disables
IME preview/candidates/learning (RFC-0075 invariant).

## Deferred

- Caret positioning within the text (v1 caret = end of value), selection,
  clipboard interactions, a11y semantics for input fields (a11y track).
