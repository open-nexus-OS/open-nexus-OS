---
title: TASK-0095 UI v15b: selection/caret engine (textedit_core) + TextField rebase + context menu + markers
status: Draft (keyboard core delivered early under TASK-0067B on 2026-10-07 — see "Delivered ahead"; the rest of this ledger is open)
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Text primitives: tasks/TASK-0094-ui-v15a-text-primitives-uax-bidi-hittest.md
  - IME/Text-input baseline (v3b): tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md
  - Clipboard v3: tasks/TASK-0087-ui-v13a-clipboard-v3.md
---

## Delivered ahead (2026-10-07, under TASK-0067B — the clipboard's board round)

The operator's board test needed Ctrl+A / Shift+arrows / Ctrl+C / Ctrl+V in text fields, so
the KEYBOARD core of this task landed with the clipboard:

- `userspace/ui/textedit_core` (crate `nexus-textedit`): char-indexed caret + anchor
  selection (`Edit`), the commands Left/Right/Home/End, their Select variants, SelectAll,
  Backspace, Delete; `insert` replaces a selection; `selected_text`, `delete_selection`.
- The DSL runtime's focused field runs on it (`View::edit_text`, `selected_text`,
  `cut_selection`; typing lands at the caret); app-host paints the caret and the selection
  highlight; the keys arrive as imed editing actions (RFC-0075 amendment, actions 4..=16).

Still open here (unchanged scope): affinity/direction, word and line navigation, pointer caret
placement and drag selection, double/triple click, the context menu, input types, the
`textedit: selection on` / `textfield: core on` markers, the TextField widget rebase.

## Context

Once we have robust hit-testing and segmentation (v15a), we can implement a reusable selection engine
and rebase `TextField` on it for consistent behavior (desktop+mobile).

IME and spellcheck integrate later (v15c/v15d).

## Goal

Deliver:

1. `userspace/ui/textedit_core`:
   - caret + selection model (affinity, direction)
   - word/line navigation (keyboard)
   - mouse/touch drag selection
   - double-click word / triple-click line selection
   - paint spans: selection highlight, composition underline placeholder, spell underline placeholder
2. TextField rebase:
   - use `textedit_core` for caret/selection
   - context menu: cut/copy/paste/select all (paste-as-plain option)
   - input types: text/search/password/number/email (stubs where needed)
3. Markers:
   - `textedit: selection on`
   - `textfield: core on`
4. Host tests for selection behavior and keybindings.

## Non-Goals

- Kernel changes.
- Full rich text (v15e).
- IME and candidate UI (v15c).

## Constraints / invariants

- Deterministic selection behavior for test event sequences.
- Bounded memory/state (caps on selection spans and history).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v15b_host/`:

- selection engine:
  - double/triple click selects word/line deterministically
  - keyboard word-jump and extend selection matches goldens
- TextField:
  - password field obscures display
  - context menu ops mutate model correctly

### Proof (OS/QEMU) — gated

UART markers:

- `textedit: selection on`
- `textfield: core on`

## Touched paths (allowlist)

- `userspace/ui/textedit_core/` (new)
- `userspace/ui/kit/` TextField (rebase)
- `tests/ui_v15b_host/`
- `docs/dev/ui/foundations/rendering/text-stack.md` (extend)

## Plan (small PRs)

1. textedit_core engine + host tests
2. TextField integration + context menu + markers
3. docs and OS selftest hook (later tasks use it)
