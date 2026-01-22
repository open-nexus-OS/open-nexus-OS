---
title: TRACK Core Utilities (Calculator + Clock + Voice Memos): “every device is usable on day 1” reference apps
status: Draft
owner: @ui
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Zero-Copy App Platform (content/grants/share): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Share v2 / Intents (registry + dispatch): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - Share v2 (targets + senders + selftests): tasks/TASK-0128-share-v2c-app-senders-selftests-postflight-docs.md
  - Files app (Open With / save/export): tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
  - Accessibility wiring (Settings + app hardening): tasks/TASK-0118-ui-v20e-accessibility-settings-app-wiring-os-proofs.md
  - NexusMedia SDK (audio record/playback for Voice Memos): tasks/TRACK-NEXUSMEDIA-SDK.md
---

## Goal (track-level)

Ship a set of first-party **Core Utilities** that make a device immediately usable:

- **Calculator**
- **Clock** (alarms/timers/stopwatch/world clock)
- **Voice Memos** (record → trim → share/export)

These are intentionally small, but they prove OS completeness, accessibility, share/open-with, and bounded storage behavior.

## Scope boundaries (anti-drift)

- No cloud sync requirements in v0.
- No “pro audio editor” features in Voice Memos (DAW track owns that).
- No kernel changes.

## App 1: Calculator

### Scope

- Standard mode (basic ops).
- Scientific mode (trig/log, constants).
- Programmer mode (base conversions, bitwise ops) optional.

### Determinism + tests

- pure compute core with property tests for numeric invariants
- deterministic formatting (locale-safe policy; bounded precision)

## App 2: Clock

### Scope

- World clock (timezone list; search).
- Alarms (repeat rules; labels; volume; snooze).
- Timers (multiple) + Stopwatch (laps optional).

### OS integration

- notification/alarm scheduling integration (policy-gated; auditable)
- bounded background behavior (no unbounded wakeups)

## App 3: Voice Memos

### Scope

- record from mic authority (permission-gated)
- list recordings (recent-first)
- trim/crop (simple non-destructive window)
- export/share (audio file via `content://` + scoped grants)

### Constraints

- no secrets in logs
- bounded recording duration and file size (configurable caps)

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-UTIL-000: Calculator app v0 (standard + scientific; host tests)**
- **CAND-UTIL-010: Clock app v0 (alarms/timers/world clock; OS markers)**
- **CAND-UTIL-020: Voice Memos app v0 (record/trim/export; permissions + share)**
