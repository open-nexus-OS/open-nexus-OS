---
title: TASK-0226 Settings v2b (OS/QEMU): Settings UI wiring (DSL) + setting:// deep links + searchable settings entries + guided setup cards + selftests/docs
status: Draft
owner: @ui
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Typed settings substrate (host-first): tasks/TASK-0225-settings-v2a-host-settingsd-typed-prefs-providers.md
  - SystemUI→DSL Settings baseline: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - SystemUI→DSL OS wiring baseline: tasks/TASK-0122-systemui-dsl-migration-phase2b-os-wiring-postflight-docs.md
  - Search execution router + setting:// URIs: tasks/TASK-0152-search-v2-ui-os-deeplinks-selftests-postflight-docs.md
  - Search backend sources: tasks/TASK-0153-search-v2-backend-host-index-ranking-analyzers-sources.md
  - Policy v1.1 prompts/dashboard: tasks/TASK-0168-policy-v1_1-os-runtime-prompts-privacy-dashboard-cli.md
  - L10n locale switching (Settings consumer): tasks/TASK-0175-l10n-i18n-v1b-os-locale-switch-settings-cli-selftests.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic Settings experience that supports:

- stable navigation and **deep links**,
- **searchable Settings entries**,
- and guided “setup cards” driven by real state (no fake success).

To avoid URI drift, Settings deep links must reuse the existing `setting://...` convention already used by Search UI.

## Goal

Deliver:

1. Settings deep links (canonical):
   - `setting://home`
   - `setting://display/scale`
   - `setting://ime/languages`
   - `setting://notifications/dnd`
   - `setting://privacy/kill/camera`
   - resolver behavior:
     - open page + optional focus/scroll to control (deterministic)
   - NOTE: do not introduce `settings://` in v2; keep a single canonical scheme.
2. Searchable settings entries:
   - expose Settings entries to `searchd` as a **settings source**:
     - documents derived from `settingsd.schema()` + Settings page metadata (titles/descriptions/tags)
     - each result includes a `setting://...` URI
   - deterministic ranking and tie-breakers (stable ordering)
3. Settings UI (DSL pages) binding to `settingsd`:
   - a small set of real pages wired to typed keys:
     - Display: `display.scale`
     - Input: `ime.locales`, `ime.personalization`
     - Notifications: `notifications.dnd.*`
     - Privacy: `privacy.kill.*` (wires to v1.1 privacy kill switches)
     - System: locale link to `TASK-0175` (avoid duplication)
   - errors show deterministic toast; no “ok” markers unless set/apply succeeds
   - markers (rate-limited):
     - `ui: settings open page=<...>`
     - `ui: settings change ns=<...> scope=<...>`
     - `ui: settings deeplink <uri>`
4. Guided setup cards (deterministic rules):
   - shown on Settings home when:
     - `ime.locales` empty → “Choose Keyboard languages”
     - privacy kill switches all false but defaults are `ask` and no decision taken (uses policyd/permsd state)
   - dismiss is persisted per-user via settingsd (no parallel “prefs” file)
   - markers:
     - `settings-guide: shown id=<...>`
     - `settings-guide: resolve id=<...>`
5. OS selftests (bounded):
   - deep link open + change a key → marker
   - settings entries appear in search results and activate → marker
   - guided card resolves and disappears → marker
   - required markers:
     - `SELFTEST: settings deeplink ok`
     - `SELFTEST: settings search ok`
     - `SELFTEST: settings guide ok`
6. Docs:
   - `docs/settings/overview.md` (scopes/types/providers)
   - `docs/settings/deeplinks.md` (setting:// canonical)
   - `docs/settings/search.md` (settings source integration)
   - update `docs/dev/ui/testing.md`

## Non-Goals

- Kernel changes.
- Inventing a new settings router parallel to Search’s `searchexec` routing model.
- A full “guided OOBE” replacement (cards are helper UX only).

## Constraints / invariants (hard requirements)

- Deterministic, offline operation in QEMU.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success markers; actions must actually execute.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - host tests from v2a (`settings_v2_host`) plus DSL host tests from `TASK-0121` where relevant
- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - required markers:
    - `SELFTEST: settings deeplink ok`
    - `SELFTEST: settings search ok`
    - `SELFTEST: settings guide ok`

## Touched paths (allowlist)

- `userspace/systemui/dsl/pages/settings/` (extend)
- `userspace/systemui/dsl_bridge/` (settings bindings)
- `source/services/settingsd/` (OS wiring use only)
- `source/services/searchd/` (settings source adapter, if needed)
- `source/apps/selftest-client/`
- `docs/settings/`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. deep link resolver (setting://) + markers
2. settings search source integration (settings entries) + markers
3. guided cards + persistence + markers
4. selftests + docs + marker contract updates

## Acceptance criteria (behavioral)

- In QEMU, Settings deep links work, settings entries are searchable and activate, and guided cards resolve deterministically.
