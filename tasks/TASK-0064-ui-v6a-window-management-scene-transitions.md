---
title: TASK-0064 UI v6a: window management in windowd (z-order/focus/states) + scene transitions (crossfade/slide)
status: Draft
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v2a present/input baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - UI v2a embedded runtime/reactor floor: tasks/TASK-0056C-ui-v2a-present-input-perf-latency-coalescing.md
  - UI v5a transitions baseline: tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md
  - UI v4a perf/pacing baseline: tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - Config broker (WM knobs): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy as Code (WM guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a minimal but robust window management layer inside `windowd` before we can talk about
“apps”, “recents”, and navigation. This task focuses on:

- a WM controller (states, z-order, focus),
- deterministic focus rules consistent with input routing,
- scene transitions (crossfade/slide) reduced-motion-aware.

The WM baseline must be visibly interactive in QEMU: live pointer focus/raise/close
must work through `windowd`, while move/resize can remain a bounded v0 if `TASK-0070`
is still pending.

This task must continue the same embedded runtime/reactor track established by
`TASK-0056C` and extended by `TASK-0062`; window management and scene transitions
should not introduce a separate event/present loop beside the fast-lane floor.

App lifecycle and notifications are handled in `TASK-0065`.

## Goal

Deliver:

1. WM controller in `windowd`:
   - window model (id, app_id, surface_id, bounds, state, z, title)
   - stacks: overlays > notifications > apps > desktop
   - actions: open/close/focus/minimize/maximize/fullscreen (move/resize stub)
   - live pointer focus/raise/close for at least the proof windows
   - use visible proof windows on the shared desktop/test surface, not detached WM-only fixtures
2. WM IPC:
   - `wm.capnp` (Open/BindSurface/SetState/Close)
3. Scene transitions engine:
   - crossfade and slide
   - handshake Prepare→Start→Commit
   - reduced motion support (disable/shorten)
4. Host tests and OS markers.

## Non-Goals

- Kernel changes.
- App lifecycle broker / recents (TASK-0065).
- Full resize semantics.

## Constraints / invariants (hard requirements)

- Deterministic focus/z-order behavior.
- Live pointer focus/raise/close must use the same hit-test/focus authority as `TASK-0056B`.
- WM transitions must reuse the existing runtime/reactor floor and preserve no-damage/idle-quiet behavior whenever scene state is unchanged.
- Bounded state:
  - cap window count,
  - cap transition duration and memory used for snapshots.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v6a_host/`:

- open two windows, bind surfaces, assert z-order and focus rules
- live QEMU pointer can focus/raise and close a proof window visibly
- close window removes it from stacks
- transitions:
  - crossfade and slide run expected frame counts/latency bounds (simulated time)
  - reduced motion disables/shortens deterministically

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: wm on`
- `windowd: wm open (app=..., win=...)`
- `windowd: wm focus (win=...)`
- `windowd: wm live focus ok`
- `windowd: wm live close ok`
- `windowd: transition on`
- `windowd: transition start (kind=crossfade|slide)`
- `windowd: transition done (ms=...)`
- `SELFTEST: ui v6 switch ok`

### Visual proof — required

- the shared proof surface contains at least two visible windows/cards with real focus/z-order changes,
- live pointer focus/raise/close is visible on that same desktop/test screen,
- scene transitions animate between visible proof surfaces instead of off-screen-only state.

## Touched paths (allowlist)

- `source/services/windowd/` (WM + transitions)
- `source/services/windowd/idl/` (`wm.capnp` and any layer extensions)
- `tests/ui_v6a_host/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v6a.sh` (delegates)
- `docs/dev/ui/patterns/wm.md` + `docs/dev/ui/foundations/transitions.md` (new)

## Plan (small PRs)

1. WM model + markers + basic IDL on the embedded runtime/reactor floor
2. focus/z-order rules + host tests
3. transitions engine + reduced motion + host tests
4. OS selftest markers + docs + postflight
