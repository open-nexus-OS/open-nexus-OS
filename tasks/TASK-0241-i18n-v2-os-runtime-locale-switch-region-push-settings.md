---
title: TASK-0241 i18n v2b (OS/QEMU): runtime locale switch via OP_SURFACE_REGION push + Settings language picker + de catalogs
status: Done (2026-07-21)
owner: @runtime
created: 2025-12-29
updated: 2026-07-21 (DONE — OS loop landed; deltas: packs ride the NXLC payload (no `locales/<tag>.nxlp` load-on-switch — all catalogs parsed once at mount), selftest marker is `SELFTEST: i18n switch ok` (settings-leg round-trip; the app re-render marker is `apphost: locale <tag> applied`). Originally rewritten: no l10nd — windowd relays settingsd changes to app-host PackLocaleSource; architecture per RFC-0077)
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
  - `SELFTEST: i18n switch ok` — `ui.locale` flips (en-US, then back to the
    shipped default de-DE) arrive as pushed OP_EVENTs
  - `apphost: locale <tag> applied` (per-surface apply line, bounded ≤8)
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

## Result (2026-07-21)

- windowd subscribes `ui.locale` as a SECOND watch on its one push channel:
  `cap_clone` the SEND half BEFORE the first `OP_WATCH` cap-move — each moved
  cap is its own subscriber slot (no wire or WatchTable change);
  `RegionState.apply` folds the tag into the `OP_SURFACE_REGION` push
  (attach + change). `source/services/windowd/src/compositor/runtime/region.rs`.
- app-host: container split at mount (`probe/locale.rs`), `app_locale!`
  source (`CatalogOverBaked`) at every dispatch site, `apply_locale` in
  `probe/clock.rs` (exact tag → primary subtag; swap + `view.reemit()` +
  relayout + bounded marker). Structure-gate splits: `probe/env.rs` (tokens +
  device env) out of `main.rs`.
- Settings → Allgemeine Verwaltung: language chips (Deutsch/English →
  `ui.locale`); `i18n/de.json` completed for settings (new), greeter +
  desktop-shell (gaps filled).
- Trap found (2026-07-22): pre-mount readers must NEVER parse the raw
  payload as a program — the window-intent reader did and greeter/shell fell
  back to floating windows (fixed via `payload_nxir`). And the attach-time
  region push was dropped by the pre-mount event-channel drains (fresh
  mounts stayed English/default-tz until the next settings change) — fixed
  by stashing (`boot::RegionPush`) + applying right after mount.
- Trap found: the first NXLC layout put the NXIR at offset 12 and broke the
  bundle payload invariants (`len % 8`, capnp 8-alignment) →
  `APPHOST: FAIL payload (header status)`. Fixed normatively in RFC-0077:
  16-byte container header + zero tail padding to 8.
- Proofs: `SELFTEST: i18n switch ok` in `just ci-os-smp1`;
  `apphost: locale de-DE applied` at greeter attach + live language switch in
  a visible boot. Gates: `just check`, `just test-host`, `just diag-os`,
  `just ci-os-smp1` green.
