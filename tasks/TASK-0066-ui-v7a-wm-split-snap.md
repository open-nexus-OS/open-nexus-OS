---
title: TASK-0066 UI v7a: multi-window split/snap zones + simple tiling policy (windowd WM)
status: Draft
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - UI v6a WM baseline: tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - UI v3a layout baseline (for future tiling): tasks/TASK-0058-ui-v3a-layout-wrapping-deterministic.md
  - Policy as Code (WM constraints): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (WM keys): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
---

## Rebase (2026-08-14) — residual-only

### Shipped elsewhere — do NOT re-implement

TASK-0070 (Done) shipped the snap baseline this draft assumed was missing (its
rewrite note, lines 21-23, records that 0066's zones never landed and were
rebuilt pointer-driven):

- `source/services/windowd/src/snap.rs` — `LeftHalf`/`RightHalf`/`Fullscreen`
  edge snapping, `SNAP_EDGE_PX = 4`, **pointer-only by design** (global
  keyboard snap shortcuts were explicitly rejected); geometry unit tests live
  inline in its `#[cfg(test)] mod tests`.
- `source/services/windowd/src/compositor/runtime/wm.rs` (476 LOC) — WM runtime.
- Z-order SSOT `source/services/windowd/src/window_scene.rs`
  (`WindowId::App(u8)`, `MAX_APP_WINDOWS = 4`).
- `dock.rs` no longer exists (deleted by RFC-0086); minimize goes to the shell
  taskbar via `OP_SURFACE_WINDOWS` (27) / `OP_SURFACE_TASKBAR` (28)
  (`source/libs/nexus-display-proto/src/surface_windows.rs`).

### Honest residual scope (Size S)

1. **Thirds** (left/center/right) on top of the shipped halves, with min-size
   compliance and fallback rules.
2. **Zone-occupancy map** (zone → window bookkeeping).
3. **Reflow on display resize** for snapped windows.
4. **`list()` IDL** (enumerate windows + snap state). Explicit
   `snap(win, zone)`/`unsnap(win)` verbs are NOT residual — pointer-driven
   snapping is the accepted design.
5. **Policy deny path** (fail-closed snap denial + reason).

### Boundary rule (docs/dev/ui/windowd-cleanup-map.md:4-9)

windowd = Single Present Authority (compositor SERVICE). WM **geometry**
(zone rects, occupancy, reflow) legitimately lives in windowd; any tiling
**UI** (zone highlights, pickers, drag visuals) would not — that belongs to
widgets / the DSL shell app (`userspace/apps/desktop-shell`). Check the
cleanup map before touching any windowd file; never build into a MOVE/DELETE
file.

### Corrected proof home

`tests/ui_v7a_host/` never existed. Follow snap.rs's existing organization:
pure geometry (thirds rects, occupancy) in inline `mod tests`;
integration-shaped cases (reflow, policy deny) in
`source/services/windowd/tests/` next to `damage_pipeline.rs`/`headless.rs`.

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

Inline `snap.rs` `mod tests` + `source/services/windowd/tests/` (corrected
2026-08-14; `tests/ui_v7a_host/` never existed):

- thirds zone rects deterministic for given display bounds (halves already covered)
- occupancy map: snap two windows → zones tracked; unsnap restores previous bounds
- resize display → snapped windows reflow deterministically
- policy deny case (multi-window disabled or min size too large) returns deny + reason

### Proof (OS/QEMU) — gated

UART markers:

- `windowd: wm split on`
- `windowd: wm snap (win=..., zone=...)`
- `windowd: wm unsnap (win=...)`
- `SELFTEST: ui v7 snap ok`

## Touched paths (allowlist) — corrected 2026-08-14

- `source/services/windowd/src/snap.rs` (thirds + occupancy) +
  `src/compositor/runtime/wm.rs` (reflow) — check the cleanup map first
- wire ops for `list()` (windowd has no `idl/*.capnp`; ops live in the wire
  libs — `source/libs/**` is an approval zone)
- `policies/` + `schemas/policy/` (wm constraints, if not already present)
- `source/services/windowd/tests/` (integration proofs)
- `source/apps/selftest-client/`
- `docs/dev/ui/patterns/wm-snap.md`

## Plan (small PRs)

1. zone definitions + rect computation + markers
2. IDL changes and WM snap/unsnap implementation
3. policy constraints integration
4. host tests + OS selftest markers + docs + postflight
