---
title: TASK-0094 UI v15a: text primitives upgrade (UAX#14/#29 + bidi UAX#9) + hit-testing + markers
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v2b shaping baseline: tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - UI v3a wrapping baseline: tasks/TASK-0058-ui-v3a-layout-wrapping-deterministic.md
  - A11y baseline: tasks/TASK-0061-ui-v4b-gestures-a11y-semantics.md
---

## Context

UI v15 requires robust text editing. Before we build selection, IME, and rich text,
we need a solid text layout/shaping boundary:

- grapheme/word boundaries (UAX#29),
- line breaking (UAX#14),
- bidi levels and reordering (UAX#9 basic),
- stable hit-testing APIs.

## Goal

Deliver:

1. `userspace/text` modules:
   - grapheme and word segmentation (UAX#29 subset)
   - line breaking (UAX#14 subset; deterministic)
   - bidi (UAX#9 basic levels/reordering; fallback to LTR when uncertain)
2. Extend shaped text API:
   - `hit_test_point(x,y) -> CaretPos`
   - `hit_test_text_pos(idx) -> Rect`
   - next/prev grapheme/word/line navigation helpers
   - selection range representation (affinity + direction)
3. Markers:
   - `text: uax on`
   - `text: bidi on`
4. Host tests:
   - segmentation and bidi hit-test goldens for mixed LTR/RTL strings.

## Non-Goals

- Kernel changes.
- Full ICU-level correctness.
- Selection engine and editable widgets (v15b).

## Constraints / invariants

- Deterministic results across runs for the same inputs (explicit rounding rules).
- Bounded processing (caps on text length per operation).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v15a_host/`:

- UAX#29 grapheme/word boundary cases match goldens
- UAX#14 line break opportunities match goldens for fixture strings
- bidi caret hit-tests for mixed LTR/RTL match goldens (caret positions and rects)

## Touched paths (allowlist)

- `userspace/text/` (new/extend)
- `tests/ui_v15a_host/`
- `docs/ui/text-stack.md` (new; v15 umbrella doc, but started here)

## Plan (small PRs)

1. segmentation + line break tables/subset + deterministic APIs
2. bidi basic implementation + hit-testing helpers
3. host tests + docs + markers
