---
title: TASK-0241 i18n/L10n v1.0b (OS/QEMU): l10nd service + Settings language page + nx-l10n CLI + hot-reload + selftests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - i18n core (host-first): tasks/TASK-0240-i18n-l10n-v1_0a-host-catalog-compiler-icu-lite-plurals-deterministic.md
  - L10n/i18n v1b baseline (Fluent + Settings): tasks/TASK-0175-l10n-i18n-v1b-os-locale-switch-settings-cli-selftests.md
  - Settings v2 (provider keys): tasks/TASK-0225-settings-v2a-host-settingsd-typed-prefs-providers.md
  - State persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/QEMU wiring for i18n/L10n v1.0:

- `l10nd` service for locale management and translation,
- Settings language page for locale switching,
- `nx l10n` CLI for extract/compile/coverage,
- hot-reload on locale change.

The prompt proposes `l10nd` as a new service, while `TASK-0175` already plans locale switching via prefs + broadcast. This task extends the existing architecture with a dedicated `l10nd` service that uses compiled catalogs (`.lc`) from `TASK-0240`, complementing or replacing the Fluent-based system from `TASK-0174/0175`.

## Goal

On OS/QEMU:

1. **l10nd service** (`source/services/l10nd/`):
   - loads compiled catalogs from `pkg://i18n/catalogs/<locale>.lc`
   - API (`l10n.capnp`): `get`, `set`, `translate`, `pluralCat`, `fmtDate`, `fmtNumber`, `bidiMark`, `coverage`
   - ICU-lite plural rules and formatting (delegates to host-first core from `TASK-0240`)
   - notifies subscribers via `samgr` topic `i18n.locale.changed`
   - markers: `l10nd: ready`, `l10nd: set locale=de-DE`, `l10nd: translate key=…`, `l10nd: coverage total=… missing=…`
2. **Settings language page** (`settings://language`):
   - lists available locales (EN/DE + `qps-ploc`)
   - switching calls `l10nd.set()` and persists `ui.locale` via `settingsd`
   - shows coverage meter for selected locale (from `l10nd.coverage()`)
   - markers: `ui: language open`, `ui: language set=de-DE`
3. **SystemUI & apps integration**:
   - replace hardcoded UI strings in SystemUI shell, Settings, Files, Store, Greeter/Lock with `i18n!()` macro
   - on locale change, hot-reload labels via event subscription
   - markers: `ui: l10n hot-reload components=k`
4. **nx l10n CLI** (subcommand of `nx`):
   - `extract`, `compile`, `pseudo`, `coverage`
   - markers: `nx: l10n extract found=m`, `nx: l10n compile locale=… keys=n`
5. **Settings key (provider)**:
   - `ui.locale` (string, user) default `en-US`; provider applies via `l10nd.set()`
6. **OS selftests + postflight**.

## Non-Goals

- Fluent format support (handled by `TASK-0174/0175`).
- Full ICU4X datasets (ICU-lite only).
- Font fallback (handled by `TASK-0174`).

## Constraints / invariants (hard requirements)

- **No duplicate locale authority**: `l10nd` is the single authority for locale management. If `TASK-0175` already implements locale switching, this task must extend or replace it, not create a parallel system.
- **Determinism**: locale switching, translation, and formatting must be stable given the same inputs.
- **Bounded resources**: compiled catalogs are size-bounded; coverage metrics are bounded.
- **`/state` gating**: persistence is only real when `TASK-0009` exists.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (locale authority drift)**:
  - Do not create a parallel locale service that conflicts with `TASK-0175` (prefs + broadcast). Extend or replace the existing architecture.
- **YELLOW (Fluent vs compiled catalogs)**:
  - If both Fluent (`TASK-0174/0175`) and compiled catalogs (`TASK-0240/0241`) coexist, they must share locale resolution and not conflict. Document the relationship explicitly.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- i18n core: `TASK-0240`
- L10n/i18n baseline: `TASK-0175` (Fluent + Settings)

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `l10nd: ready`
- `l10nd: set locale=de-DE`
- `l10nd: translate key=…`
- `l10nd: coverage total=… missing=…`
- `ui: language open`
- `ui: language set=de-DE`
- `ui: l10n hot-reload components=k`
- `SELFTEST: i18n plural de ok`
- `SELFTEST: i18n format ok`
- `SELFTEST: i18n pseudolocale ok`
- `SELFTEST: i18n switch hot-reload ok`

## Touched paths (allowlist)

- `source/services/l10nd/` (new)
- `userspace/libs/i18n/` (new; runtime helper + `i18n!()` macro)
- SystemUI (string replacement + hot-reload)
- Settings (language page)
- `source/services/settingsd/` (extend: `ui.locale` provider key)
- `tools/nx/` (extend: `nx l10n ...` subcommands)
- `source/apps/selftest-client/` (markers)
- `pkg://fixtures/i18n/` (source catalogs + compiled `.lc` + month names)
- `docs/i18n/overview.md` (new)
- `docs/tools/nx-l10n.md` (new)
- `tools/postflight-i18n-v1_0.sh` (new)

## Plan (small PRs)

1. **l10nd service**
   - load compiled catalogs (`.lc`)
   - API: get/set/translate/pluralCat/fmtDate/fmtNumber/bidiMark/coverage
   - notify subscribers on locale change
   - markers

2. **Settings language page + provider**
   - language page UI
   - `ui.locale` provider key
   - coverage meter
   - markers

3. **SystemUI & apps integration**
   - replace hardcoded strings with `i18n!()` macro
   - hot-reload on locale change
   - markers

4. **nx l10n CLI + selftests**
   - CLI: extract/compile/pseudo/coverage
   - OS selftests + postflight

## Acceptance criteria (behavioral)

- `l10nd` manages locale and translation correctly.
- Settings language page switches locale with hot-reload.
- SystemUI strings are localized correctly.
- All four OS selftest markers are emitted.
