---
title: TASK-0148 Text stack v1 (deterministic): bidi/line-break contract + shaping integration for editing
status: Draft
owner: @ui
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Shaping baseline (HarfBuzz): tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - Text primitives baseline (UAX): tasks/TASK-0094-ui-v15a-text-primitives-uax-bidi-hittest.md
  - IME v2 Part 1: tasks/TASK-0146-ime-text-v2-part1a-imed-keymaps-host.md
  - IME v2 Part 2 engines: tasks/TASK-0149-ime-v2-part2-cjk-engines-userdict.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

IME v2 Part 2 needs a deterministic “text stack” substrate:

- bidi runs for cursor movement and hit-testing,
- line-break opportunities for layout and editing (not full ICU correctness),
- shaping outputs suitable for rendering and caret anchoring.

The repo already tracks shaping (`TASK-0057`) and UAX primitives (`TASK-0094`).
This task defines the **integration contract** between them and the editing/IME layers, with deterministic fixtures.

## Goal

Deliver:

1. A stable API surface (crate placement aligned with existing tasks):
   - `userspace/text/*` owns:
     - bidi runs (UAX#9 basic)
     - segmentation (UAX#29 subset)
     - line-break opportunities (UAX#14 subset)
   - `userspace/ui/shape/*` owns:
     - shaping (HarfBuzz host-first, OS-enabled only when feasible)
     - output types for renderer (`GlyphRun`, clusters, advances)
2. Integration contract for editing:
   - map from byte/char indices → grapheme indices → shaped cluster positions
   - caret rect anchoring model that IME UI can rely on
3. Deterministic fixture suite:
   - mixed LTR/RTL cursor paths
   - CJK line-break fixture strings
   - shaping fixture strings with documented tolerance policy
4. Markers (throttled; debugging only):
   - `text: bidi on`
   - `text: breaks on`
   - `ui.shape: hb=off` (when HarfBuzz is not used on OS path)

## Non-Goals

- Kernel changes.
- Full ICU correctness (explicit subset).
- Shipping ICU4X data blobs via `pkg://` in this step unless already required elsewhere (see red flag).

## Constraints / invariants (hard requirements)

- Determinism: identical inputs produce identical outputs; where tolerances are required, they are explicit and tested.
- Bounded processing: caps on text length per operation and table sizes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (ICU4X data size / OS viability)**:
  - “Embed ICU4X segmenter data and load from `pkg://`” may be too heavy and may not fit no_std/alloc constraints.
  - Default plan: implement a deterministic UAX#14 subset in `userspace/text` (already scoped in `TASK-0094`).
  - If ICU4X is still desired later, make it a separate, host-first data-pipeline task with explicit size budgets.

- **YELLOW (HarfBuzz in OS)**:
  - `TASK-0057` already flags this: keep HarfBuzz host-first; OS path may use a simpler shaper or precomputed runs.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p ui_v15a_host -- --nocapture` (or equivalent test crate produced by `TASK-0094`)
    - `cargo test -p ui_v2b_host -- --nocapture` (or equivalent from `TASK-0057`)
  - Required coverage additions:
    - caret anchoring and cursor movement goldens for mixed LTR/RTL
    - line-break fixture goldens for CJK strings

## Touched paths (allowlist)

- `userspace/text/`
- `userspace/ui/shape/`
- `tests/ui_v15a_host/` (fixtures additions)
- `tests/ui_v2b_host/` (fixtures additions)
- `docs/dev/ui/text-stack.md` (canonical text-stack umbrella doc for UI + DSL consumers)

## Plan (small PRs)

1. Add the editing integration contract types (caret anchoring, cluster mapping)
2. Add deterministic fixture goldens and tests
3. Update docs with “subset + determinism policy” and explicit non-goals

## Acceptance criteria (behavioral)

- Host tests cover bidi/line-break/shaping integration needed by IME candidate UI anchoring.
- No OS/QEMU markers are required for this step (host-first substrate).

Follow-up:

- Text v2.1 (hyphenation patterns, grapheme/emoji cluster safety, persistent glyph cache under `/state`, renderer atlas upload hooks, and metrics surfaces) is tracked as `TASK-0201`/`TASK-0202`.
