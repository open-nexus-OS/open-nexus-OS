---
title: TASK-0066 UI v7a: multi-window split/snap zones + simple tiling policy (windowd WM)
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v6a WM baseline: tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - UI v3a layout baseline (for future tiling): tasks/TASK-0058-ui-v3a-layout-wrapping-deterministic.md
  - Policy as Code (WM constraints): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (WM keys): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With UI v6 we have a basic WM. UI v7a adds productive “multi-window” behavior:

- snap zones (halves/thirds),
- simple tiling map (zone → window),
- reflow on display resize,
- and a policy hook to restrict multi-window per app.

DnD/clipboard/screencap/share are explicitly out of scope here (v7b/v7c).

## Goal

Deliver:

1. Snap zones in `windowd` WM:
   - left/right/top/bottom halves; left/center/right thirds
   - min-size compliance and fallback rules
2. WM IDL extensions:
   - `snap(win, zone)`, `unsnap(win)`, `list()`
3. Simple tiling policy:
   - maintain zone occupancy
   - reflow snapped windows on display resize
4. Markers + host tests + OS/QEMU markers (gated).

## Non-Goals

- Kernel changes.
- Full tiling WM and dynamic layouts.
- Drag-and-drop, clipboard, screenshot/share (separate tasks).

## Constraints / invariants (hard requirements)

- Deterministic zone rect computation for a given display bounds.
- Bounded WM state (cap max windows).
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Policy enforcement is fail-closed (deny snap if not allowed).

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v7a_host/`:

- open two windows, snap left/right halves → bounds equal zone rects
- unsnap restores previous bounds
- resize display → snapped windows reflow deterministically
- policy deny case (multi-window disabled or min size too large) returns deny + reason

### Proof (OS/QEMU) — gated

UART markers:

- `windowd: wm split on`
- `windowd: wm snap (win=..., zone=...)`
- `windowd: wm unsnap (win=...)`
- `SELFTEST: ui v7 snap ok`

## Touched paths (allowlist)

- `source/services/windowd/` + `idl/wm.capnp` (snap/unsnap)
- `policies/` + `schemas/policy/` (wm constraints, if not already present)
- `tests/ui_v7a_host/`
- `source/apps/selftest-client/`
- `tools/postflight-ui-v7a.sh` (delegates)
- `docs/ui/wm-snap.md`

## Plan (small PRs)

1. zone definitions + rect computation + markers
2. IDL changes and WM snap/unsnap implementation
3. policy constraints integration
4. host tests + OS selftest markers + docs + postflight
