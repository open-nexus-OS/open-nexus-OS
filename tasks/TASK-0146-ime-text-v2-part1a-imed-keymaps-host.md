---
title: TASK-0146 IME/Text v2 Part 1a (host-first): imed core + US/DE keymaps + dead/compose + deterministic tests
status: Draft
owner: @ui
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - IME/text-input plumbing baseline: tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md
  - Later IME candidate UI/OSK: tasks/TASK-0096-ui-v15c-ime-candidate-ui-osk.md
  - Policy focus guards (future): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic IME foundation for UI text controls:

- focus/session state,
- a preedit/commit pipeline,
- US/DE keymaps including dead keys and a small compose table,
- deterministic host tests.

Repo reality today:

- `TASK-0059` already plans an `imed` **stub** as part of broader UI work. This task extracts the IME logic
  and keymaps into a dedicated, testable slice.
- Full CJK engines, shaping/bidi, and rich candidate UI are deferred (follow-up tasks).
- There is an existing placeholder service crate at `source/services/ime/` that currently prints `ime: ready`
  and performs a trivial uppercase transform. IME v2 standardizes on **`imed` as the canonical IME authority**
  (see `tasks/TRACK-AUTHORITY-NAMING.md`). The placeholder must be retired, renamed, or turned into a thin shim
  to avoid parallel IME daemons and marker drift.
  - Repo reality closure: the first IME v2 implementation step is to **rename/replace** `source/services/ime/`
    to `source/services/imed/` so authority + markers match the registry.

## Goal

Deliver:

1. `imed` core service logic (host-first):
   - language selection (US/DE for Part 1)
   - compose/dead-key state machine
   - preedit buffer and commit output
   - bounded candidate list snapshot (can be empty in Part 1)
2. `userspace/libs/ime-keymaps`:
   - US/DE scancode→keysym mapping
   - modifiers including AltGr (DE)
   - dead keys + deterministic compose table for common sequences (`" + a → ä`, etc.)
   - pure-Rust Latin transliteration helper (no ICU/HarfBuzz in Part 1)
3. Deterministic host tests proving:
   - mapping (US/DE) for representative keys (`@` via AltGr, `ß`, etc.)
   - compose/dead-key sequences yield expected Unicode
   - preedit→commit event ordering is deterministic

## Non-Goals

- Kernel changes.
- Full IME engines (JP/KR/ZH), dictionaries, or language models (Part 2).
- Full shaping/bidi/line breaking (Part 2).
- On-screen keyboard UI (Part 1b).
- Low-level input device drivers (HID/touch) or input event pipeline (handled by `TASK-0252`/`TASK-0253`; this task focuses on IME keymaps for IME engine).

## Constraints / invariants (hard requirements)

- Determinism: tests and markers are stable; no timing-based assertions.
- Bounded state: max preedit length, max compose sequence length, max candidates.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **YELLOW (IPC contract choice)**:
  - A full callback-cap interface (`ImeClient`) can require capability transfer and careful reentrancy.
  - Part 1 should prefer a **simple, deterministic return value** from `keyEvent` (e.g. `ImeOutcome { handled, preedit, commit }`)
    for host tests and early OS bring-up. A callback/stream contract can be added later once cap transfer semantics are proven.

## Contract sources (single source of truth)

- **QEMU marker contract**: `scripts/qemu-test.sh` (OS markers added only in Part 1b)
- **UI input routing contract**: `TASK-0056`/`TASK-0059` (focus routing + Editable hooks)

## Stop conditions (Definition of Done)

- **Proof (tests / host)**:
  - Command(s):
    - `cargo test -p ime_v2_part1_host -- --nocapture` (or equivalent crate name)
  - Required coverage:
    - US/DE mapping + compose/dead-key sequences
    - preedit/commit ordering

## Touched paths (allowlist)

- `source/services/imed/` (core; host-first implementation)
- `userspace/libs/ime-keymaps/` (new)
- `tests/ime_v2_part1_host/` (new)
- `docs/input/` (added in Part 1b)

## Plan (small PRs)

1. `ime-keymaps` library + deterministic compose tables + unit tests
2. `imed` core state machine + deterministic outcome API + tests

## Acceptance criteria (behavioral)

- Host tests prove that US/DE key sequences produce deterministic preedit/commit results.
- No OS/QEMU markers are claimed in this part.
