---
title: TASK-0066 UI v7a: windowd WM zones — halves + thirds, occupancy map, reflow on display-mode change, snap state in the window feed, fail-closed policy
status: Draft (end-state rewrite 2026-09-09 — halves shipped by TASK-0070; residual = thirds/occupancy/reflow/feed/policy/markers)
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

## End-state rewrite 2026-09-09 (binding; supersedes older sections where they differ)

**Ground truth 2026-09-09 (verified in code):** halves + fullscreen pointer snap exist
(`source/services/windowd/src/snap.rs`, 109 LOC, `SnapTarget::{LeftHalf,RightHalf,Fullscreen}`,
`SNAP_EDGE_PX = 4`, driven by `apply_release_snap` in `compositor/runtime/wm.rs:365-398`,
TASK-0070 Done); window enumeration ships as the feed `OP_SURFACE_WINDOWS = 27` /
`OP_SURFACE_TASKBAR = 28` (`nexus-display-proto/src/surface_windows.rs`, `WindowEntry{owner_sid,
flags}`, 9 B/entry, RFC-0086) — no snap state in it. Missing: thirds + min-size rules, a zone
occupancy map, reflow on display-mode/work-area change, policy deny, registered markers
(`windowd: snap edge=…` is an ungated `debug_println`), and `docs/dev/ui/patterns/wm-snap.md`
is a 10-line stub that claims keyboard snapping (rejected design; pointer-only is accepted).
**Verify first:** the 2026-07-29 finding "chrome drag to the top edge + release →
`toggle_fullscreen` → window invisible, every later window invisible" (transition/override
wedge) has no fix in the git log since — reproduce on the visible lane before P1; if it still
wedges, fixing it is P1's first commit (it is the same code path).

### Goal (end system)

windowd's WM owns snap geometry, occupancy and reflow for halves and thirds, pointer-driven
only; snap state is part of the one window feed; denial is fail-closed with a named reason;
every marker is registered. Zone-picker UI (if ever) is a shell/widget concern, never windowd.

### Non-goals

Keyboard snap shortcuts (rejected design); explicit `snap()/unsnap()/list()` verbs (the feed IS
`list()`); zone highlight/picker UI; dynamic tiling layouts; kernel changes.

### Invariants

- Zone rects are a pure function of `(mode_w, work_area_h, top)`; ≤ `MAX_APP_WINDOWS`
  occupancy entries; ONE geometry-apply path (`apply_window_frame`).
- The policy verdict comes from the single presentation resolver
  (`surface_presentation.rs`), no policyd round trip in the input path.
- Deterministic transitions; no `unwrap`/`expect`; no blanket allows.

### Decisions

- **D1 Thirds are occupancy-driven, not a new gesture.** Edge release → the half if free; if
  that half is occupied the map re-partitions to thirds (previous occupant moves inward to
  `CenterThird`, the newcomer takes the outer third, the opposite half becomes its outer
  third); dragging a snapped window off its zone unsnaps it and the map collapses back to
  halves; a fourth window with all three zones taken → `ZonesFull` deny; `mode_w/3 < min
  width` → `MinSize` deny (halves kept).
- **D2 `snap.rs` REPLACED by `zones.rs`** (`Zone {LeftHalf, RightHalf, LeftThird, CenterThird,
  RightThird, Fullscreen}`, `ZoneMap` = occupancy + layout mode + prior frames for restore,
  `zone_frames(mode_w, work_h, top)`); `snap.rs` deleted in the same package. Gate: `wm.rs`
  compiles only against `zones::ZoneMap`; `config/loc-baseline.txt` never re-lists `snap.rs`.
- **D3 Snap state rides the existing feed** (RFC-0086 append-only flag bits):
  `WINDOW_ZONE_SHIFT = 2`, `WINDOW_ZONE_MASK = 0b111 << 2`, `ZONE_NONE..ZONE_FULLSCREEN = 0..6`;
  wire layout unchanged (9 B/entry). RFC-0086 amended (approval zone). app-host
  `effect_windows.rs::window_state_of` exposes `zone` to the shell.
- **D4 Reflow trigger = work-area change** (shell-mode tablet/desktop toggle, and the display
  mode TASK-0324 P6 makes authoritative): `wm.rs::reflow_snapped()` re-applies `ZoneMap` frames
  through `apply_window_frame`.
- **D5 Deny = presentation verdict.** `resizable = false` or role ≠ `Window` →
  `SnapDeny::NotResizable`; reasons `{NotResizable, ZonesFull, MinSize}` are a stable enum and
  marker token.
- **D6 Ungated prints DELETED** (`wm.rs:376-393`) and replaced by registered literals in
  `windowd/src/markers.rs`.

### Packages

- **P0** Wedge reproduction on the visible lane + RFC-0086 amendment (zone bits) +
  `wm-snap.md` rewritten (pointer-only, occupancy rule). Blast: paper (+ the wedge fix if
  reproduced: windowd transitions; lanes visible, smp1).
- **P1** `zones.rs` + inline tests (thirds rects incl. odd widths, occupancy transitions,
  restore). Blast: windowd host tests.
- **P2** `wm.rs` integration (release → `ZoneMap`, reflow, deny) + `source/services/windowd/
  tests/snap_zones.rs`. Blast: `ui_windowd_host`, lanes smp1 + visible.
- **P3** Feed bits + app-host decode + markers registered + selftest probe. Blast: RFC-0086
  consumers (desktop-shell taskbar), `check-chain-markers`.

### Definition of Done

Host: thirds/halves rects deterministic; two- then three-window occupancy → thirds; unsnap
restores the prior frame; work-area change reflows; deny table (`test_reject_not_resizable`,
`test_reject_zones_full`, `test_reject_min_size`). QEMU (registered in `proof-manifest/markers/
ui.toml`, `scripts/qemu-test.sh` full/visible lists, `tools/nx/chains/markers.txt` group
`wm-snap` + `tools/nx/src/chain/contract/windowd.rs`): `windowd: wm split on`,
`windowd: wm snap (zone=left-half id=…)`, `windowd: wm snap (zone=center-third id=…)`,
`windowd: wm unsnap (id=…)`, `windowd: wm snap deny (reason=zones-full)`,
`SELFTEST: ui v7 snap ok`. Docs: `wm-snap.md`, `docs/testing/os-markers.md`.

### Touched paths

`source/services/windowd/src/{zones.rs (new),snap.rs (deleted),compositor/runtime/wm.rs,
surface_presentation.rs,markers.rs,compositor/runtime/windows_feed.rs}`,
`source/services/windowd/tests/snap_zones.rs`, `source/libs/nexus-display-proto/src/
surface_windows.rs` (approval zone), `source/services/app-host/src/effect_windows.rs`,
`docs/rfcs/RFC-0086-*.md` (approval zone), markers triple, `source/apps/selftest-client/src/
os_lite/`.

### Dependencies

TASK-0324 P4a (windowd on the declarative arm) and P6 (display mode from the kernel = the
reflow source of truth).

## Rebase (2026-08-14) — residual-only — historical, superseded by the end-state rewrite above

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

## Context — historical, superseded by the end-state rewrite above

With UI v6 we have a basic WM. UI v7a adds productive “multi-window” behavior:

- snap zones (halves/thirds),
- simple tiling map (zone → window),
- reflow on display resize,
- and a policy hook to restrict multi-window per app.

DnD/clipboard/screencap/share are explicitly out of scope here (v7b/v7c).

## Goal — historical, superseded by the end-state rewrite above

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
