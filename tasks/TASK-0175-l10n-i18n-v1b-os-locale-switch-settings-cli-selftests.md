---
title: TASK-0175 L10n/i18n v1b (OS/QEMU): runtime locale switching + Settings Language/Region + nx-l10n + RTL toggle + CJK font fallbacks + selftests/docs
status: Draft
owner: @ui
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - L10n core + fontsel: tasks/TASK-0174-l10n-i18n-v1a-host-core-fluent-icu-fontsel-goldens.md
  - Renderer/windowd OS wiring: tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - Search v2 UI (i18n consumer): tasks/TASK-0151-search-v2-ui-host-command-palette-model-a11y.md
  - Media UX v1 (i18n consumer): tasks/TASK-0156-media-ux-v1b-os-miniplayer-lockscreen-sample-cli-selftests.md
  - SystemUI Settings DSL baseline: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - Prefs substrate: tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - Policy v1.1 caps (optional): tasks/TASK-0167-policy-v1_1-host-scoped-grants-expiry-enumeration.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With the host-first i18n and font fallback core in place (`TASK-0174`), we need OS integration:

- runtime locale switching and propagation to SystemUI,
- a Settings page for Language & Region,
- a CLI to inspect/set locale and run formatting tests,
- RTL dev override for deterministic testing,
- CJK font fallback wired into text rendering.

## Goal

Deliver:

1. Locale storage + broadcast:
   - store current locale in prefs (e.g. `state:/prefs/locale.json` via prefsd when present)
   - broadcast a “locale changed” signal to SystemUI
   - marker: `i18n: locale set <tag>`
2. SystemUI integration:
   - use i18n message lookup for:
     - tray clock
     - search placeholder
     - mini-player labels
     - settings headings (at least a small subset)
   - RTL handling:
     - root direction switch when locale is RTL or dev override is enabled
     - caret movement and bidi hit-testing remains gated on text stack tasks; v1b only wires the direction flag
3. Settings page: Language & Region:
   - select language/region from an allowlist
   - dev-only pseudo-locale (`xqps`)
   - previews for number/date/plural formatting
   - apply flow triggers UI reload (soft)
   - markers:
     - `settings:l10n apply lang=<tag>`
4. CLI `nx l10n`:
   - get/set locale
   - `test number/date/plural` subcommands for deterministic outputs
   - stable output for parsing + markers
5. OS selftests (bounded):
   - set locale to `de-DE` and verify a deterministic UI string path (marker-based)
   - render mixed CJK sample and require fallback markers
   - toggle RTL override and require direction marker
   - switch to `xqps` and verify pseudo-locale expansion appears
   - markers:
     - `SELFTEST: l10n de-DE clock ok`
     - `SELFTEST: l10n cjk fallback ok`
     - `SELFTEST: l10n rtl ok`
     - `SELFTEST: l10n xqps ok`
6. Docs:
   - overview, formatting rules, fonts/fallback, settings UX, testing and markers

## Non-Goals

- Kernel changes.
- Full locale coverage and full ICU datasets.
- Full RTL layout engine correctness (separate text/layout tasks).
- `l10nd` service or compiled catalog format (`.lc`) (handled by `TASK-0241` as a lightweight alternative; this task uses prefs + broadcast).

## Constraints / invariants (hard requirements)

- Deterministic locale switching (no environment-dependent locale APIs).
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: “CJK fallback ok” only if fallback was actually exercised and observed.

## Red flags / decision points (track explicitly)

- **YELLOW (prefs/prefsd availability)**:
  - If prefsd is not present yet, locale can be stored in a deterministic fallback file, but must be explicitly documented.

- **RED (font assets availability)**:
  - We need deterministic test fonts under `pkg://fonts/...`. If these are not present, this task must add minimal subset fonts as fixtures.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p l10n_v1_host -- --nocapture` (from `TASK-0174`)

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: l10n de-DE clock ok`
    - `SELFTEST: l10n cjk fallback ok`
    - `SELFTEST: l10n rtl ok`
    - `SELFTEST: l10n xqps ok`

## Touched paths (allowlist)

- SystemUI string lookup integration points (tray/search/media/settings)
- Settings DSL page for Language & Region
- `tools/nx-l10n/` (new)
- `source/apps/selftest-client/`
- `docs/l10n/`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. Locale storage + broadcast + minimal SystemUI string integration
2. Language & Region Settings page + previews
3. nx-l10n CLI
4. OS selftests + docs + marker contract

## Acceptance criteria (behavioral)

- In QEMU, locale switching is observable via deterministic markers and affects selected UI strings; CJK fallback and pseudo-locale are proven.
