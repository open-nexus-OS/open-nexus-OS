---
title: TASK-0147 IME/Text v2 Part 1b (OS-gated): OSK overlay + focus routing + a11y announcements + OS selftests/postflight + docs
status: Draft
owner: @ui
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - IME core + keymaps: tasks/TASK-0146-ime-text-v2-part1a-imed-keymaps-host.md
  - IME/text-input plumbing baseline: tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md
  - TextField core (caret/selection): tasks/TASK-0095-ui-v15b-selection-caret-textfield-core.md
  - SystemUI→DSL migration baseline: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - A11y baseline: tasks/TASK-0114-ui-v20a-a11yd-tree-actions-focusnav.md
  - Policy gates (input caps): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With IME core logic and keymaps in place (Part 1a), we need the OS-visible integration:

- focus/session wiring from windowd/ui-kit into imed,
- an on-screen keyboard overlay (OSK),
- accessibility announcements for preedit/commit,
- deterministic OS selftests and docs.

This must not duplicate the “imed stub” scope inside `TASK-0059`; instead it wires the real IME.

## Goal

Deliver:

1. Focus/session wiring:
   - `Editable` text controls negotiate IME focus with `imed`:
     - focusIn/focusOut
     - surrounding text + caret rect queries (as supported)
   - key routing:
     - focused control’s key events go to `imed` first
     - if `handled=false`, fall back to legacy handling
   - async composition model (foundation):
     - `imed` emits preedit/candidate updates as queued events per focused session (no reentrancy into UI)
     - bounded queue depth; deterministic delivery order
2. OSK overlay (SystemUI):
   - US/DE layouts
   - dead-key highlight and modifier state
   - a11y roles/names per key; optional sticky modifiers (v1)
   - markers:
     - `osk: show` / `osk: hide`
     - `osk: key "<label>"`
3. Settings integration + shortcuts + CLI:
   - Settings → Keyboard & Input (US/DE, OSK toggle, sticky modifiers)
   - shortcuts: Super+Space cycle lang; Ctrl+Space toggle OSK
   - `nx ime` helpers (lang/osk, simple deterministic typing test)
4. Policy/a11y:
   - capability gating for IME usage and IME management (`input.ime`, `input.ime.manage`)
   - screen reader announcements for preedit/commit changes (bounded)
5. Proof:
   - OS/QEMU selftests for:
     - latin typing (US)
     - dead keys/compose (DE)
     - OSK typing (e.g., `ß`)
   - postflight delegates to host tests + QEMU marker run
6. Docs:
   - `docs/input/ime-overview.md`
   - `docs/input/keymaps.md`
   - `docs/systemui/osk.md`

## Non-Goals

- Kernel changes.
- CJK engines and shaping/bidi/line-breaking (Part 2).
- Candidate UI anchored near caret (later task; see `TASK-0096` direction).

## Constraints / invariants (hard requirements)

- Deterministic markers; bounded timeouts; no busy-wait.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Focus guard correctness: only the focused editable receives commits.
 - Async IME events must not leak across focus changes.

## Red flags / decision points (track explicitly)

- **YELLOW (DSL availability)**:
  - If SystemUI DSL overlays are not yet wired in OS builds, OSK may need a minimal non-DSL overlay first.
  - This task should explicitly gate OSK-on-OS behind “SystemUI overlay infrastructure is present”.

## Contract sources (single source of truth)

- **QEMU marker contract**: `scripts/qemu-test.sh`
- **IME core behavior**: `TASK-0146`
- **Input routing/focus**: `TASK-0056`/`TASK-0059`

## Stop conditions (Definition of Done)

- **Proof (tests / host)**:
  - Command(s):
    - `cargo test -p ime_v2_part1_host -- --nocapture`

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers (to be added to `scripts/qemu-test.sh` expected list):
    - `imed: ready`
    - `SELFTEST: ime v2 latin us ok`
    - `SELFTEST: ime v2 deadkeys de ok`
    - `SELFTEST: ime v2 osk ok`

## Touched paths (allowlist)

- `source/services/imed/`
- `userspace/ui/kit/` (Editable integration)
- `source/services/windowd/` (input routing to IME)
- `userspace/systemui/overlays/osk/` (new)
- `tools/nx-ime/` (new)
- `source/apps/selftest-client/`
- `tools/postflight-ime-v2-part1.sh`
- `docs/input/` + `docs/systemui/`

## Plan (small PRs)

1. wire focus/key routing: windowd + ui-kit → imed
2. implement OSK overlay + markers
3. settings/shortcuts + nx-ime
4. selftests + postflight + docs

## Acceptance criteria (behavioral)

- OSK and physical keyboard both drive the same preedit/commit pipeline.
- US/DE compose/dead-key behavior is deterministic and tested.
- Only the focused editable receives commits; unfocused inputs do not.
