---
title: TASK-0095 UI v15b: selection/caret engine (textedit_core) + TextField rebase + context menu + markers
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Text primitives: tasks/TASK-0094-ui-v15a-text-primitives-uax-bidi-hittest.md
  - IME/Text-input baseline (v3b): tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md
  - Clipboard v3: tasks/TASK-0087-ui-v13a-clipboard-v3.md
---

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
- `docs/ui/text-stack.md` (extend)

## Plan (small PRs)

1. textedit_core engine + host tests
2. TextField integration + context menu + markers
3. docs and OS selftest hook (later tasks use it)

