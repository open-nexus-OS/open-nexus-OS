---
title: TASK-0240 i18n/L10n v1.0a (host-first): catalog compiler + ICU-lite plurals & interpolation + pseudolocale + deterministic tests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - L10n/i18n v1a baseline (Fluent + ICU4X): tasks/TASK-0174-l10n-i18n-v1a-host-core-fluent-icu-fontsel-goldens.md
  - Settings v2 (provider keys): tasks/TASK-0225-settings-v2a-host-settingsd-typed-prefs-providers.md
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

We need a lightweight i18n foundation with:

- compiled string catalogs (compact binary format),
- ICU-lite message formatting (plurals, interpolation),
- pseudolocale for testing,
- deterministic catalog compiler.

The prompt proposes compiled catalogs (`.lc`) with JSON source format and ICU-lite, while `TASK-0174` uses Fluent (`.ftl`) + ICU4X. This task delivers a **lightweight alternative** that can coexist with or complement the Fluent-based system. The catalog compiler produces compact binaries suitable for OS/no_std constraints.

## Goal

Deliver on host:

1. **Catalog compiler** (`tools/nx-l10n`):
   - source format: JSON with ICU-lite fragments (e.g., `{count, plural, one {# file} other {# files}}`)
   - produces `.lc` (compact binary, Cap'n Proto encoded):
     - schema-defined, versionable, deterministic bytes (signable if needed later)
     - interning key strings, bytecode for message AST (literals, `{var}`, `plural`, `number`)
   - deterministic ordering and stable hashing
   - CLI: `extract`, `compile`, `pseudo`, `coverage`
   - markers: `i18n-compile: en-US keys=n`, `i18n-extract: found=m`, `i18n-coverage: de-DE missing=k`
2. **ICU-lite runtime** (`userspace/libs/i18n_catalog/`):
   - plural rules for `en`, `de` (cardinal `one/other`), and generic fallback
   - interpolation: `{var}` substitution
   - number formatting: fixed grouping (`en: 1,234.5`, `de: 1.234,5`)
   - date/time formatting: deterministic templates (`short` → `YYYY-MM-DD`, `HH:MM`)
   - deterministic outputs (no host locale leakage)
3. **Pseudolocale** (`qps-ploc`):
   - widen + accent + pad: `"Open Nexus"` → `⟦ Ōpēn Nēxūs ⟧`
   - deterministic transformation
4. **Bidi helper**:
   - inserts LRM/RLM around foreign-direction spans when `dir=auto`
   - deterministic marking
5. **Host tests** proving:
   - catalog compilation produces stable `.lc` files
   - plural rules work correctly for EN/DE
   - interpolation handles variables correctly
   - number/date formatting matches templates
   - pseudolocale transformation is deterministic
   - bidi marking works correctly

## Non-Goals

- Fluent format support (handled by `TASK-0174`).
- Full ICU4X datasets (ICU-lite only).
- OS/QEMU markers (deferred to v1.0b).
- Font fallback (handled by `TASK-0174`).

## Constraints / invariants (hard requirements)

- **No duplicate i18n authority**: This task provides a lightweight alternative to Fluent (`TASK-0174`). If both systems coexist, they must share locale resolution and not conflict.
- **Determinism**: catalog compilation, plural rules, formatting, and pseudolocale must be stable given the same inputs.
- **Bounded resources**: compiled catalogs are size-bounded; formatting rules are deterministic.
- **no_std compatibility**: compiled catalogs (`.lc`) must be decodable in no_std environments.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (i18n authority drift)**:
  - Do not create a parallel i18n system that conflicts with `TASK-0174` (Fluent). If both coexist, they must share locale resolution and message lookup APIs, or this task must explicitly replace Fluent as the canonical system.
- **YELLOW (format choice)**:
  - JSON source + compiled `.lc` is lighter than Fluent but less expressive. Document trade-offs explicitly.

## Contract sources (single source of truth)

- L10n/i18n baseline: `TASK-0174` (Fluent + ICU4X)
- Settings v2: `TASK-0225` (provider keys)

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p i18n_v1_0_host` green (new):

- catalog compilation: JSON → `.lc` produces stable binaries
- plural rules: `files.count` with 1 and 5 in EN/DE → correct forms
- interpolation: `{var}` substitution works correctly
- number/date formatting: `fmtNumber(0.123,"percent")` → `12%` (EN), `12 %` (DE); `fmtDate` matches template
- pseudolocale: switch to `qps-ploc` → returned strings padded and widened
- bidi: `bidiMark("שלום", "auto")` wraps with RLM

## Touched paths (allowlist)

- `userspace/libs/i18n_catalog/` (new; ICU-lite runtime)
- `tools/nx-l10n/` (new; catalog compiler CLI)
- `schemas/i18n_v1_0.schema.json` (new)
- `tests/i18n_v1_0_host/` (new)
- `pkg://i18n/src/` (source catalogs: `en-US.json`, `de-DE.json`)
- `pkg://i18n/catalogs/` (compiled `.lc` files)
- `docs/i18n/compiler.md` (new, host-first sections)

## Plan (small PRs)

1. **Catalog compiler**
   - JSON source parser + ICU-lite fragment parser
   - `.lc` binary format (compact, deterministic)
   - CLI: extract/compile/pseudo/coverage
   - host tests

2. **ICU-lite runtime**
   - plural rules (EN/DE + fallback)
   - interpolation
   - number/date formatting
   - host tests

3. **Pseudolocale + bidi**
   - pseudolocale transformation
   - bidi marking helper
   - host tests

4. **Schema + docs**
   - `schemas/i18n_v1_0.schema.json`
   - host-first docs

## Acceptance criteria (behavioral)

- Catalog compilation produces stable `.lc` files.
- Plural rules work correctly for EN/DE.
- Interpolation handles variables correctly.
- Number/date formatting matches templates.
- Pseudolocale transformation is deterministic.
- Bidi marking works correctly.
