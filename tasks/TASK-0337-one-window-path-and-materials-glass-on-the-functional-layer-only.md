---
title: TASK-0337 One window path and the materials — every app on window-kit, windowd's title bar retired, glass only on the functional layer, the content pane opaque, controls tinted, state as a layer's opacity, reduce-transparency
status: Planned
owner: @ui
created: 2026-10-21
depends-on: [TASK-0336 (the window as one surface)]
follow-up-tasks: [TASK-0335 P2 occlusion (the opaque pane is the occluder), the programs (rounded/blur shaders)]
links:
  - Standard: docs/dev/ui/foundations/visual/materials.md (the five rules)
  - Plan: docs/dev/ui/patterns/windowing/windows-as-widgets.md (the one window construction that stays)
  - Measurement: docs/board/measurements/2026-10-21-gpu-vendor-per-pixel/README.md ("Cycle 51": the three window paths, the glass layers per window)
  - Code today: userspace/apps/window-kit/ui/components/WinAppWindow.nx, userspace/dsl/runtime/src/registry.rs (materials), source/services/app-host/src/probe/layers.rs (material → glass level), source/services/app-host/src/hover_wash.rs (the paint-time hover), source/services/windowd/src/compositor/runtime/app_window.rs (`has_chrome`, the legacy title bar)
---

## Context (cycle 51)

- **Three constructions of a window on one screen.** calculator and chat run
  `style=titlebar`: windowd draws its legacy `ShellWindow` title bar over a plain surface
  (chat past the band cap: `band too tall, plain-path fallback`, whole frames on content
  change). settings runs `style=plain` with window-kit's chrome on the plain path (`statics
  beside the viewport`), stash the same on the band path. Every cut of the frame cost is
  measured on one construction and missed on the other two.
- **Glass on the content layer.** window-kit's `windowPane` is `GLASS_CARD` (blur 20) — 70–80 %
  of every window is a glass layer with its own blur chain; the bar and the sidebar are two
  more. settings submits 2 glass layers, stash 3, chat 4; the scene carries nine
  (`glass_idx=0x3fe`), each a rounded + shadow draw at 9–11 ns a pixel.
- **State is pixels.** A hover repaints the box's rows on the CPU (`hover_wash`) and presents;
  a tap in settings resubmits the glass layers (`dmg=Some(Layout)` → `submitted 2 layers`).
- The reference platforms' rules (materials.md): glass on the functional layer only, never
  glass on glass, controls opaque or tinted, state as a property, a reduce-transparency switch.

## Goal

Every app window is the same object: window-kit's chrome in the app's surface, no title bar
from windowd, one glass layer (the chrome) over an opaque content pane, controls tinted, hover
and press as a wash layer's opacity. `submitted 1 layers` for every app; a hover presents no
pixels; `glass_w` per frame ≤ the shell's glasses + one per window.

## Non-Goals

- The planner's occlusion itself (TASK-0335 P2 — it needs this task's opaque pane).
- New chrome features (resize handles, menus): the existing window-kit set, used by all.

## Three lenses

- **Scope**: `userspace/apps/{calculator,chat}` (manifests + pages onto `WinAppWindow`,
  `style=plain`), `userspace/apps/window-kit` (materials: one glass, pane opaque, wash layer),
  `userspace/dsl/runtime` (material tokens: `tint`, the wash), `source/services/app-host`
  (the wash as a layer property, the band cap / the plain path's row damage, the layer
  resubmit only when a layer's geometry changed), `source/services/windowd` (**deletions**:
  the app title bar and `has_chrome`; the retired-names gate takes `ShellWindow`'s title),
  `settingsd` (reduce-transparency). Out: the kernel, gpud, the planner.
- **Invariant**: a window's pixels are the app's alone — windowd draws nothing inside a
  window's frame (`test_reject_a_title_bar_from_windowd`); a wash layer never carries content;
  the band path holds any content height or the plain path damages rows, never the frame
  (`test_reject_a_whole_frame_present_for_a_row_change`).
- **Contract**: windows-as-widgets.md (RFC-0067 P3/P4), materials.md, RFC-0101 §8 (`opaque`
  on the pane's layer — the hint P2 consumes).

## Plan

- **P0 — the count.** Per app on the board: `window intent style=`, the band line, `submitted N
  layers`, presents per hover (a hover series over a sidebar: `apphost: submitted` count).
- **P1 — one path.** calculator and chat onto `WinAppWindow` + `style=plain` (resizable where
  the design says); windowd's app title bar deleted (`has_chrome` and `update_app_title_overlay`
  go; the title's draw with them); chat's band: the cap lifted (the band is the viewport's rows,
  not the content's) or the plain path's row-span damage made the rule for every present.
- **P2 — the materials.** `windowPane` opaque (`opaque = true` on its layer), the chrome one
  glass (`windowBar` + sidebar as one material layer), cards and panels `tint`; the shell's
  dock/top bar/Control Center glass as today. Reduce-transparency in settingsd: every material
  its tint; app-host reads it like the accent.
- **P3 — state as a property.** The wash layer per interactive component (colour, radius; the
  DSL's hover/press/selected states set its opacity); app-host sends the opacity through the
  layer-transform op, no repaint, no present; layers resubmitted only when their geometry
  changed, never for a content row.
- **Proof**: host tests (window-kit goldens, the wash rule, the band/row rule); `just test-os
  visible` (the apps mount, the markers); board: `submitted 1 layers` for all four apps,
  `glass_w` per frame, a hover series without presents, and the visual check by the operator:
  nothing looks different, only faster.
