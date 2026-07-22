---
title: TASK-0240 i18n v2a (host-first): locale-pack compiler in nx build + PackLocaleSource + deterministic goldens
status: Done (2026-07-21)
owner: @runtime
created: 2025-12-29
updated: 2026-07-21 (DONE — host layer landed; naming deltas: pack magic `NXL1` index-aligned inline in the `NXLC` payload container (no `locales/*.nxlp` sidecar files), runtime source = `Catalog::from_indexed_pack` + `CatalogOverBaked` (no separate `PackLocaleSource` type). Originally rewritten: replaces both old i18n families — no Fluent/ICU4X (ex 0174/0175), packs ride the existing DSL catalog; architecture per RFC-0077)
depends-on:
  - TASK-0077
follow-up-tasks:
  - TASK-0241 (OS runtime switch + region push + Settings language picker)
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract seed: docs/rfcs/RFC-0077-i18n-v2-locale-packs-runtime-switch.md (seeded by this task)
  - DSL i18n core (Done): tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - Superseded heavy line: tasks/TASK-0174-l10n-i18n-v1a-host-core-fluent-icu-fontsel-goldens.md
  - Data formats rubric: docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

Repo reality: DSL apps already have compile-time i18n — `@t("key")` resolves
through NXIR `i18n_keys`, and the runtime already routes lookups through a
`LocaleSource::format(key, args)` trait (app-host mounts `IdentityLocale`).
The default locale catalog (`i18n/en.json`) is baked into the program at build.

That means runtime language switching needs **no compiler-semantics change**:
compile every `i18n/<tag>.json` into a compact binary **locale pack** shipped
next to the app, and swap the `LocaleSource` at runtime. The old plans —
Fluent + ICU4X (TASK-0174/0175, now Superseded) and an l10nd service with its
own catalog compiler (old 0240/0241) — are replaced by this minimal line:
no new daemon, no new authority, no forbidden/heavy deps.

## Goal

1. **Pack format** (versioned, deterministic): header (magic `NXLP`, version,
   locale tag, key count) + key-indexed string table (key order = NXIR
   `i18n_keys` order; strings deduped; all offsets bounded). Byte-stable
   across identical inputs (golden tests).
2. **Pack compiler** in the `nx` build (`userspace/dsl/core` + `tools/nx`):
   compiles every `i18n/<tag>.json` beside the manifest into
   `<bundle>/locales/<tag>.nxlp`. Missing key in a non-default catalog →
   pack marks it absent (fallback to baked default at runtime); malformed
   catalog → build error (same policy as today's default catalog).
3. **`PackLocaleSource`** (host-testable lib in `userspace/dsl/runtime`):
   mounts a pack buffer, serves `format(key, args)` with the existing
   interpolation semantics; fallback chain: requested tag → primary language
   tag → baked default. Bounded parsing (fail-closed on truncated packs).
4. Deterministic host goldens: pack bytes, fallback chain, reemit-on-swap
   (swap source → `view.reemit()` yields translated strings).

## Non-Goals

- No OS wiring, no windowd push, no Settings UI (TASK-0241).
- No plural rules / no ICU-lite formatter in this slice — `{0}`-style
  interpolation subset only; plurals are a documented RFC-0077 follow-up.
- No l10nd service, no Fluent, no ICU4X, no font-fallback work.
- No RTL (revisit with an RTL locale; TASK-0148 stays Deferred).

## Constraints / invariants (hard requirements)

- Determinism: identical inputs → byte-identical packs (goldens are contract).
- NXIR unchanged: packs are a **sidecar** artifact; baked default remains —
  apps without extra catalogs behave exactly as today.
- Pack parsing is fail-closed and bounded (truncation/mutation reject tests).
- no_std-compatible runtime path; no new dependencies.

## Security considerations

- Packs are untrusted at mount time (they travel with bundles): bounds-check
  header/offsets before use; reject matrix over golden packs (truncations +
  header mutations → clean fallback to baked default, never a panic).
- No format-string interpretation beyond the fixed `{n}` placeholder subset.

## Contract sources (single source of truth)

- **Pack format**: RFC-0077 + golden pack fixtures in
  `userspace/dsl/runtime/tests/` (bytes = contract).

## Stop conditions (Definition of Done)

- **Proof (host)**: pack-compiler goldens (en/de fixture app), fallback-chain
  tests, reemit-swap test, reject matrix — all green.
- **Gates**: `just check` + `just test-host` green.

## Touched paths (allowlist)

- `userspace/dsl/core/` (pack compiler), `tools/nx/` (build step)
- `userspace/dsl/runtime/` (PackLocaleSource + tests)
- `docs/rfcs/RFC-0077-*.md` (new seed) — **approval zone**
- `docs/dev/dsl/i18n.md`, `CHANGELOG.md`

## Plan (small PRs)

1. RFC-0077 seed (format + fallback + runtime-switch contract).
2. Pack format + compiler + goldens.
3. PackLocaleSource + fallback + reemit tests + reject matrix.

## Acceptance criteria (behavioral)

- `nx build` of an app with `i18n/{en,de}.json` emits two byte-stable packs;
  swapping the mounted source at runtime re-renders `@t()` strings in German
  with English fallback for missing keys — proven on host.

## Result (2026-07-21)

Landed per RFC-0077 with two naming/shape deltas vs the plan above:

- Packs are **not sidecar files** — `compile_project_bundle`
  (`userspace/dsl/core/src/locale_pack.rs`) compiles every `i18n/<tag>.json`
  into an `NXL1` index-aligned pack and ships them INLINE in the `NXLC`
  payload container (16-byte header keeps the NXIR 8-aligned; total length
  padded to a multiple of 8 — bundle-payload invariant). Pack-less apps keep
  the raw `.nxir` payload byte-identically.
- The runtime source is `Catalog::from_indexed_pack` (fail-closed `NXL1`
  parser) + `CatalogOverBaked` (active catalog → baked default terminal) in
  `userspace/dsl/runtime/src/i18n.rs` — no separate `PackLocaleSource` type.
- Proofs: `tests/dsl_goldens/tests/i18n_packs.rs` — golden bytes, round-trip,
  swap-reemit (en→de re-renders via `view.reemit()`), pack-less parity, and
  `test_reject_*` truncation/mutation matrices for packs AND containers.
  Gates: `just check` + `just test-host` green.
