---
title: TASK-0241 i18n v2b (OS/QEMU): runtime locale switch via OP_SURFACE_REGION push + Settings language picker + de catalogs
status: Draft
owner: @runtime
created: 2025-12-29
updated: 2026-07-21 (rewritten: no l10nd — windowd relays settingsd changes to app-host PackLocaleSource; architecture per RFC-0077)
depends-on:
  - TASK-0240
  - TASK-0298
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract: docs/rfcs/RFC-0077-i18n-v2-locale-packs-runtime-switch.md
  - Pack compiler + PackLocaleSource: tasks/TASK-0240-i18n-l10n-v1_0a-host-catalog-compiler-icu-lite-plurals-deterministic.md
  - Settings spine (ui.locale + watch): tasks/TASK-0298-settings-spine-watch-region-keys.md
  - Superseded heavy line: tasks/TASK-0175-l10n-i18n-v1b-os-locale-switch-settings-cli-selftests.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

TASK-0240 makes locale packs and the swappable `PackLocaleSource` host-real.
This task wires the OS loop: the `ui.locale` settingsd key (registered since
TASK-0072, **no consumer until now**) drives live UI language via the same
propagation pattern as theme — windowd watches settingsd and pushes to
surfaces. No l10nd service (old plan dropped): settingsd is the value
authority, windowd the relay, app-host the applier.

## Goal

1. windowd: watch `ui.` / `time.` / `region.` via settingsd `OP_WATCH`
   (TASK-0298) and push `OP_SURFACE_REGION` (locale tag, tz, hour format) to
   every surface at create + on change — routing only, no locale logic.
2. app-host: on `OP_SURFACE_REGION` locale change → load the bundle's
   `locales/<tag>.nxlp` (fallback chain per RFC-0077), swap
   `PackLocaleSource`, `view.reemit()` — live re-render without app restart.
3. Settings app: General management → real Language picker (en/de first),
   writing `ui.locale` via `svc.settings.set`.
4. German catalogs: author `i18n/de.json` for settings and any launcher/shell
   strings still en-only (greeter + desktop-shell de.json exist).
5. Selftest: set `ui.locale=de-DE` → observe a known widget re-render with
   the German string (fixture key), then back to en.

## Non-Goals

- No l10nd daemon, no hot-reload of catalogs from disk (packs are bundle
  artifacts), no per-app language override.
- No plural rules / RTL (RFC-0077 follow-ups).
- No date/number formatting — that lands with the clock work (TASK-0297,
  tz-lite formats the shell clock; broader CLDR-style formatting deferred).

## Constraints / invariants (hard requirements)

- Propagation is push-based (no polling loops); one relay hop
  (settingsd → windowd → surfaces) — apps do not talk to settingsd for locale.
- Pack load is bounded + fail-closed (falls back to baked default; a broken
  pack can never take an app down).
- Reemit is a full-scene re-emit — acceptable at human-triggered frequency;
  never wired to any per-frame path.
- Markers honest; marker changes ride qemu-test.sh + markers.txt + docs.

## Security considerations

- `OP_SURFACE_REGION` accepted by apps only from windowd (existing push
  channel identity); settingsd validates locale tags before storing
  (existing BCP-47-ish validator) — no attacker-controlled tag reaches path
  construction beyond `[a-zA-Z-]` (re-validated at pack lookup, fail-closed).

## Contract sources (single source of truth)

- **Propagation contract**: RFC-0077 + display-proto golden tests
  (`OP_SURFACE_REGION` frame).
- **QEMU marker contract**: `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt`.

## Stop conditions (Definition of Done)

- **Proof (QEMU)**:
  - `SELFTEST: i18n switch de ok` — locale set → German string observed
  - `apphost: locale de-DE applied` (per-surface apply line, bounded)
- **Proof (interactive)**: `just start` — switch language in Settings, whole
  UI (settings/shell/greeter strings) re-renders in German instantly.
- **Gates**: `just check`, `just test-all` green; RFC-0077 checklist ticked;
  task + RFC documented Done.

## Touched paths (allowlist)

- `source/services/windowd/` (watch + region push; routing only)
- `source/services/app-host/` (pack mount + reemit)
- `source/libs/nexus-display-proto/` (`OP_SURFACE_REGION=23`) — **approval zone**
- `userspace/apps/settings/` (language picker + de.json), other app `i18n/de.json`
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`, `tools/nx/chains/markers.txt` — **approval zone**
- `docs/dev/dsl/i18n.md`, `CHANGELOG.md`

## Plan (small PRs)

1. `OP_SURFACE_REGION` codec + windowd watch/relay + app-host apply + selftest.
2. Settings language picker + de catalogs + interactive proof + docs.

## Acceptance criteria (behavioral)

- Language change in Settings re-renders all running DSL apps live, with
  English fallback for untranslated keys; persists across reboot via the
  existing prefs blob.
