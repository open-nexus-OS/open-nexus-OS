---
title: TASK-0061 UI v4b: gesture recognizers + inertial scroll + accessibility semantics tree + focus navigation/events
status: Done
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
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

Gestures must build on the live QEMU pointer path from `TASK-0253`, the visible
affordance semantics from `TASK-0056B`, and the scroll path from `TASK-0059`;
synthetic sequences remain host/regression coverage only.
The gesture proof should stay on the shared visible proof surface, reusing the small scroll/gesture window instead of
creating a detached gesture-only demo.

## Goal

Deliver:

1. Gesture recognizer module:
   - tap, double-tap, long-press, pan, fling (pinch stub)
   - deterministic thresholds and time windows
   - inertial scroll integrator
2. `windowd` integration:
   - gestures run before delivery and can produce higher-level events
   - fling feeds scroll offsets
   - live pointer pan/fling over the proof surface produces visible movement in QEMU
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
- Live input posture:
  - host pointer move/down/up is accepted only through the established input path,
  - gesture recognizers do not become a second hit-test/focus authority.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Security posture:
  - deny remote semantics injection by default (policy guarded)
  - only local UI subjects can register trees

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v4b_host/`:

- gestures: event sequences → expected recognition (tap/double/long/pan/fling) and velocities
- inertial scroll: fling produces monotonic decay and bounded scroll deltas
- live QEMU pointer drag/fling proof updates a visible scroll/gesture surface
- a11y: register tree, focus next/prev, activate → expected event log

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: gestures on`
- `a11yd: ready`
- `a11yreader: on`
- `windowd: fling ok (vx=..., vy=...)`
- `windowd: live gesture ok`
- `a11y: focus cycle ok`
- `SELFTEST: ui v4 fling ok`
- `SELFTEST: ui v4 a11y focus ok`

### Visual proof — required

- the shared proof surface exposes a visible gesture/scroll target,
- pan/fling visibly moves that target on-screen,
- a11y focus movement is tied to visible UI targets rather than marker-only events.

## Touched paths (allowlist)

- `userspace/ui/gesture/` (new)
- `source/services/windowd/` (gesture integration; emit markers)
- `source/services/a11yd/` (new)
- `tests/ui_v4b_host/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v4b.sh` (delegates)
- `docs/dev/ui/input/gestures.md` + `docs/accessibility/semantics.md` (new)

## Plan (small PRs)

1. gestures module + deterministic thresholds
2. windowd integration + inertial scroll
3. a11yd service + reader stub + focus navigation
4. tests + docs + postflight

## Closure (2026-07-19) — Reconciliation
Core DoD met and boot-proven: gesture recognizers, inertial scroll, accessibility semantics tree + focus/keyboard navigation (UI v4b markers). Remaining a11y-semantics *hardening* (actions surface, adapter coverage) is folded into **TASK-0114** (a11yd tree/actions/focusnav), which already lists this task as its a11y baseline. Status → Done.
