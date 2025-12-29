---
title: TASK-0096 UI v15c: IME service (latin+pinyin stub+emoji) + candidate UI + on-screen keyboard + focus guards
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Text primitives: tasks/TASK-0094-ui-v15a-text-primitives-uax-bidi-hittest.md
  - TextField core: tasks/TASK-0095-ui-v15b-selection-caret-textfield-core.md
  - Policy as Code (focus guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (ime/osk defaults): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic input pipeline:

- IME service that can produce composition + candidates + commits,
- candidate UI anchored near caret,
- on-screen keyboard (OSK) that emits key events through the IME pipeline.

This task focuses on IME+OSK. Spellcheck and rich text are separate tasks.

Scope note:

- IME/Text v2 Part 1 (`TASK-0146`/`TASK-0147`) delivers the **foundation** (imed host + US/DE keymaps + OSK baseline + focus routing).
- This task builds on that with richer engines (pinyin/emoji), candidate UI, and more advanced OSK behaviors.
- CJK engines + user dictionary persistence are tracked as IME v2 Part 2 (`TASK-0149`/`TASK-0150`).

## Goal

Deliver:

1. `imed` service:
   - IDL `ime.capnp` and `key()` API returning handled/commit/composition
   - modes:
     - latin (dead keys)
     - pinyin stub (small deterministic dictionary; candidates)
     - emoji shortcodes (e.g., `:smile:`)
   - markers:
     - `imed: ready`
     - `ime: comp show`
     - `ime: commit "<text>"`
2. Candidate UI overlay (SystemUI):
   - list of candidates near caret
   - navigation and selection via arrows/Enter or numeric shortcuts
3. OSK plugin:
   - TOML-defined layouts
   - desktop (floating) + mobile (dock) modes
   - show/hide rules: show when touch focuses editable; tray toggle
   - markers:
     - `osk: ready`
     - `osk: show`
     - `osk: hide`
4. Focus guards:
   - IME events delivered only to focused editable
   - policy markers (optional): `policy: ime focus-guard on`
5. Host tests for IME candidate selection and OSK key emission sequences.

## Non-Goals

- Kernel changes.
- Full production IME engines (language models).
- Full international keyboard layouts (minimal set only).

## Constraints / invariants

- Deterministic candidate ordering and selection behavior.
- Bounded input buffers and candidate list size.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v15c_host/`:

- IME pinyin stub input sequence yields deterministic candidates and commit
- OSK layout emits deterministic KeyEvents; diacritics long-press emits composed grapheme

### Proof (OS/QEMU) — gated

UART markers:

- `imed: ready`
- `osk: ready`
- `SELFTEST: ui v15 ime ok`
- `SELFTEST: ui v15 osk ok`

## Touched paths (allowlist)

- `source/services/imed/` (new or extend if already exists as stub)
- SystemUI candidate overlay + OSK plugin
- `tests/ui_v15c_host/`
- `docs/ui/ime.md` (new)

## Plan (small PRs)

1. extend imed (build on IME v2 Part 1) with deterministic pinyin/emoji + markers
2. candidate overlay + focus routing
3. OSK plugin + layouts + markers
4. host tests + docs
