---
title: TASK-0061 UI v4b: gesture recognizers + inertial scroll + accessibility semantics tree + focus navigation/events
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v4a perf baseline: tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - UI v2a input routing baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - UI v3b IME/text-input baseline (focus/caret): tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md
  - Policy as Code (a11y injection guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (thresholds): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

After UI v4a improves frame efficiency, v4b improves input ergonomics and accessibility:

- gesture recognizers (tap/double/long/pan/fling) with deterministic thresholds,
- inertial scrolling integrated with scroll layers,
- accessibility semantics tree + focus navigation + events, via an `a11yd` service and a reader stub.

## Goal

Deliver:

1. Gesture recognizer module:
   - tap, double-tap, long-press, pan, fling (pinch stub)
   - deterministic thresholds and time windows
   - inertial scroll integrator
2. `windowd` integration:
   - gestures run before delivery and can produce higher-level events
   - fling feeds scroll offsets
3. Accessibility service `a11yd`:
   - register/update semantics tree (roles/labels/states/bounds)
   - focus next/prev + activate
   - event stream (focus changed, announcement)
   - reader stub prints events (no TTS)
4. Host tests and OS selftest markers.

## Non-Goals

- Kernel changes.
- Full screen reader/TTS.
- Full gesture set (pinch remains stub).

## Constraints / invariants (hard requirements)

- Deterministic gesture recognition:
  - stable timeouts and distance thresholds
  - stable velocity computation and decay
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Security posture:
  - deny remote semantics injection by default (policy guarded)
  - only local UI subjects can register trees

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v4b_host/`:

- gestures: event sequences → expected recognition (tap/double/long/pan/fling) and velocities
- inertial scroll: fling produces monotonic decay and bounded scroll deltas
- a11y: register tree, focus next/prev, activate → expected event log

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: gestures on`
- `a11yd: ready`
- `a11yreader: on`
- `windowd: fling ok (vx=..., vy=...)`
- `a11y: focus cycle ok`
- `SELFTEST: ui v4 fling ok`
- `SELFTEST: ui v4 a11y focus ok`

## Touched paths (allowlist)

- `userspace/ui/gesture/` (new)
- `source/services/windowd/` (gesture integration; emit markers)
- `source/services/a11yd/` (new)
- `tests/ui_v4b_host/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v4b.sh` (delegates)
- `docs/ui/gestures.md` + `docs/accessibility/semantics.md` (new)

## Plan (small PRs)

1. gestures module + deterministic thresholds
2. windowd integration + inertial scroll
3. a11yd service + reader stub + focus navigation
4. tests + docs + postflight
