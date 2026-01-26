---
title: TASK-0174 L10n/i18n v1a (host-first): locale resolver + Fluent bundles + ICU4X formatting + pseudo-locale + font fallback resolver + goldens
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Renderer abstraction (font integration): tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - Text stack contract (bidi/line-break/shaping): tasks/TASK-0148-textshape-v1-deterministic-bidi-breaks-shaping-contract.md
  - Shaping baseline (font fallback chain): tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md
  - DSL i18n runtime plan (avoid duplicate semantics): tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic localization framework that supports:

- locale resolution and runtime switching,
- message lookup with fallbacks,
- plural rules + number/date formatting,
- RTL signaling,
- and a deterministic font fallback chain for CJK.

Repo reality:

- There is no general OS i18n library yet.
- DSL has its own planned i18n runtime (`TASK-0077/0078`) which should reuse or align with this core to avoid duplication.
- Renderer abstraction tasks (`TASK-0169/0170`) need a font fallback resolver to handle mixed scripts deterministically.

This task is host-first: we prove correctness with deterministic host tests and renderer goldens.
OS wiring (Settings/CLI/selftests) is in `TASK-0175`.

## Goal

Deliver:

1. `userspace/libs/i18n` (or equivalent shared crate):
   - `LocaleResolver`:
     - expands locale chains deterministically (`de-DE → de-DE,de,en`)
     - `is_rtl(lang)` helper
   - Fluent message bundles:
     - load `.ftl` packs from `pkg://i18n/<locale>/*.ftl`
     - deterministic fallback ordering and deterministic error strings on missing keys
   - ICU4X formatting helpers:
     - numbers, dates (UTC-only in tests), plural categories
     - deterministic output rules (no host locale leakage)
   - pseudo-locale `xqps`:
     - deterministic expansion/bracketing to catch truncation
   - markers (throttled):
     - `i18n: bundles loaded locales=[...]`
2. Seeded resource packs (fixtures):
   - `pkg://i18n/en/core.ftl`, `de`, `ja`, `ko`, `zh`, `xqps` (short, test-ready)
3. `userspace/libs/fontsel`:
   - deterministic font fallback selection by codepoint + language/script hints
   - minimal CJK fallback mapping suitable for tests (subset fonts)
   - API returns a stable “font ref” (not raw filesystem paths)
   - markers (throttled):
     - `fontsel: fallback U+4E2D -> <font>`
4. Renderer/text integration (host-first):
   - `renderer`/`textshape` can request fallback fonts on missing glyphs deterministically
   - host golden scene renders mixed text (`"中国 한글"`) and matches a golden PNG hash

## Non-Goals

- Kernel changes.
- Shipping full ICU datasets or full locale coverage (minimal set only).
- Full RTL shaping correctness beyond the existing text stack plan (bidi/hit-test remains `TASK-0094/0148` scope).
- Shipping a second, parallel runtime format for OS:
  - **Canonical runtime catalogs are `.lc` (Cap'n Proto encoded)** per `TASK-0240`/`TASK-0241`.
  - This task focuses on **authoring semantics** (Fluent + ICU4X) and host proofs; OS uses compiled `.lc` catalogs.

## Constraints / invariants (hard requirements)

- Deterministic outputs across runs and machines.
- Bounded resource packs and bounded formatter data.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (ICU4X data size / OS viability)**:
  - ICU4X “real” data blobs can be large and may be hard to support in OS/no_std builds.
  - v1a is host-first; OS enablement must be gated on an explicit data size budget and build feasibility.

- **YELLOW (Fluent in OS)**:
  - Fluent runtime may be `std`-heavy. If OS cannot support it, we may need:
    - a **compiler pipeline**: Fluent (`.ftl`) → compiled `.lc` (Cap'n Proto encoded; deterministic), keeping Fluent as authoring format,
    - or a smaller authoring format for some domains (JSON catalogs) while still compiling to `.lc` for runtime.
  - **Do not** make OS load `.ftl` directly unless the feasibility/data budgets are explicitly proven.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p l10n_v1_host -- --nocapture`
  - Required tests:
    - locale chain expansion and RTL detection
    - Fluent lookup with fallback behavior
    - plural/number/date formatting fixtures (minimal locales)
    - pseudo-locale behavior
    - font fallback selection determinism
    - renderer golden for mixed-script sample

## Touched paths (allowlist)

- `userspace/libs/i18n/` (new)
- `userspace/libs/fontsel/` (new)
- `pkg://i18n/` (fixture assets; exact repo path to be chosen)
- `tests/l10n_v1_host/` (new)
- `docs/l10n/` (added in `TASK-0175` or here if minimal)

## Plan (small PRs)

1. Locale resolver + pseudo-locale + host tests
2. Fluent loader + fixture packs + host tests
3. ICU4X format helpers + fixtures + host tests (host-only if needed)
4. fontsel + renderer/text integration + golden snapshots

## Acceptance criteria (behavioral)

- Host tests prove deterministic locale resolution, message lookup, formatting, and font fallback behavior.
