---
title: TASK-0118 UI v20e: accessibility settings pages + app wiring hardening + OS selftests/postflight/docs
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - A11y daemon + focus: tasks/TASK-0114-ui-v20a-a11yd-tree-actions-focusnav.md
  - Screen reader: tasks/TASK-0115-ui-v20b-screen-reader-ttsd-earcons.md
  - Magnifier/filters/HC: tasks/TASK-0116-ui-v20c-magnifier-colorfilters-highcontrast.md
  - Captions: tasks/TASK-0117-ui-v20d-captions-subtitles.md
  - Prefs store: tasks/TASK-0072-ui-v9b-prefsd-settings-panels-quick-settings.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

v20a–v20d deliver the pieces. v20e wires them into a coherent user-facing suite:

- Accessibility settings pages (toggle reader, magnifier, filters, captions),
- harden a11y exports in key apps,
- add OS selftests and postflight markers.

## Goal

Deliver:

1. Settings → Accessibility section:
   - Screen Reader: enabled, rate/pitch/volume, verbosity
   - Display: color filter preset, high contrast toggle
   - Magnifier: mode/zoom
   - Captions: font size/background and global toggle
   - Keyboard: tab order preview (stub)
   - marker: `settings:a11y apply (k=...,v=...)`
2. App wiring hardening:
   - ensure SystemUI, Launcher, Files, Settings, Text, Browser export roles/names and respond to `doAction`
   - focus changes emit a11y focus events and earcons when reader enabled
3. OS selftests:
   - focus across controls emits `a11y: focus` events → `SELFTEST: ui v20 focus ok`
   - enable magnifier lens + high contrast → `SELFTEST: ui v20 display ok`
   - open Video with sample captions and toggle CC → `SELFTEST: ui v20 captions ok`
   - trigger reader speak on focused button → `SELFTEST: ui v20 reader ok`
4. Postflight `postflight-ui-v20.sh` (delegating) and docs.

## Non-Goals

- Kernel changes.
- Full TTS voice quality.

## Constraints / invariants

- Deterministic markers and bounded selftests.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `a11yd: ready`
- `readerd: ready`
- `ttsd: ready`
- `SELFTEST: ui v20 focus ok`
- `SELFTEST: ui v20 display ok`
- `SELFTEST: ui v20 captions ok`
- `SELFTEST: ui v20 reader ok`

## Touched paths (allowlist)

- `userspace/apps/settings/` (extend)
- `source/apps/selftest-client/`
- `tools/postflight-ui-v20.sh` (delegates)
- `docs/a11y/overview.md` + `docs/a11y/vision.md` + `docs/a11y/screen-reader.md` (extend)
- `docs/ui/testing.md` (extend)

## Plan (small PRs)

1. settings pages + prefs wiring + marker
2. app wiring hardening (roles/actions/focus events)
3. OS selftests + postflight + docs

