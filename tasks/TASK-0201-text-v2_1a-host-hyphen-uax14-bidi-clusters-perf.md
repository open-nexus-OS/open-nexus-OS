---
title: TASK-0201 Text v2.1a (host-first): hyphenation (EN/DE) + UAX#14 line breaks + UAX#9 bidi runs + grapheme/emoji clusters + deterministic perf benchmarks
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Text primitives (UAX#14/#29 + bidi UAX#9 + hit-test): tasks/TASK-0094-ui-v15a-text-primitives-uax-bidi-hittest.md
  - Text stack integration contract (bidi/breaks/shaping): tasks/TASK-0148-textshape-v1-deterministic-bidi-breaks-shaping-contract.md
  - Shaping baseline (HarfBuzz host-first): tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - Renderer abstraction goldens (consumer): tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
---

## Context

We already track core text primitives and the integration contract (`TASK-0094`, `TASK-0148`).
Text v2.1 adds:

- hyphenation patterns (EN/DE) and hyphen insertion decisions,
- grapheme clustering with emoji ZWJ sequence handling (no color emoji yet),
- shaping/layout perf counters and deterministic “bench gates” on host.

This task stays host-first to keep proofs deterministic and independent of `/state` and OS services.

## Goal

Deliver:

1. `userspace/textbreak` (or `userspace/text/*` module) extensions:
   - UAX#14 subset improvements for mixed Latin+CJK punctuation
   - hyphenation:
     - TeX-style patterns for `en` and `de-1996` from fixtures
     - `hyphen_points(word, locale)` deterministic
     - line break hints include `HintKind::Hyphen` with explicit penalty rules
2. `userspace/textbidi` (or `userspace/text/*` module) extensions:
   - UAX#9 “basic” with stable run output and documented fallbacks
   - stable punctuation mirroring for the supported subset
3. Grapheme cluster boundaries:
   - extended grapheme cluster boundaries (subset acceptable, but explicit)
   - ZWJ emoji sequences treated as unbreakable clusters
   - breaks/hyphenation must never split a cluster
4. Deterministic perf counters + host “bench” tests:
   - stable inputs/width/px produce stable shaped-glyph counts and stable line box hashes
   - perf metrics must be recorded, but pass/fail gates must not be flaky:
     - use relative comparisons (e.g., second run faster or fewer misses) rather than absolute time unless time is simulated

## Non-Goals

- Persistent glyph cache and atlas pages on disk (v2.1b; `/state` gated).
- Renderer atlas upload hooks (v2.1b).
- Full ICU-level correctness.

## Constraints / invariants (hard requirements)

- Deterministic outputs for fixed inputs.
- Bounded processing: caps on input length, max break hints, max runs.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **YELLOW (time-based perf gates)**:
  - Avoid absolute “ms < X” assertions unless time is simulated/injected. Prefer stable counters and cache-hit deltas.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p text_v2_1_host -- --nocapture`
  - Required:
    - hyphenation fixture words match expected indices
    - UAX#14 line-break hints match goldens for mixed LTR/RTL/CJK fixtures
    - bidi runs stable for mixed-direction fixtures
    - grapheme clusters stable for emoji ZWJ fixtures
    - deterministic layout “quad hash” snapshot for a fixed paragraph

## Touched paths (allowlist)

- `userspace/text/` (extend)
- `pkg://fixtures/text/` (new: paragraphs + hyphen pattern fixtures)
- `tests/text_v2_1_host/` (new)
- `docs/text/linebreak.md` (added in v2.1b or minimal here)

## Plan (small PRs)

1. fixtures + hyphenation pattern loader + unit tests
2. UAX#14 subset refinements + golden tests
3. bidi runs + mirroring subset + golden tests
4. grapheme clusters + emoji ZWJ handling + tests
5. deterministic “bench” snapshot hashing and counters (time-injected or counter-based)

## Acceptance criteria (behavioral)

- Host tests deterministically prove hyphenation, line breaks, bidi runs, and cluster safety for emoji ZWJ.
