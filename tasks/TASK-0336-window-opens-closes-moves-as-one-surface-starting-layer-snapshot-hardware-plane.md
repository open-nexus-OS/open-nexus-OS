---
title: TASK-0336 A window opens, closes and moves as ONE surface — the starting layer at the tap, the snapshot on close and drag, the dragged window on a display-controller plane, glass blurred at rest
status: Planned
owner: @ui @gpu
created: 2026-10-21
depends-on: [TASK-0335 (P3 ✅ cycle 51), TASK-0333 (6a ✅ — a death is an event)]
follow-up-tasks: [TASK-0337 one window path + materials, TASK-0335 P2 occlusion, the non-parking compositor (ADR-0075), 6c the launch without a copy]
links:
  - RFC: docs/rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md (§8: the planner; the snapshot source and the plane bind are its extension)
  - RFC: docs/rfcs/RFC-0108-ux-latency-contract-instant-and-smooth-as-numbers.md (the budgets this meets)
  - Measurement: docs/board/measurements/2026-10-21-gpu-vendor-per-pixel/README.md ("Cycle 51": the open sequence, the drag window, the stock controller's planes)
  - Standard: docs/dev/ui/foundations/visual/materials.md (rule 4, blur at rest)
  - Code today: source/services/windowd/src/compositor/runtime/transitions.rs (the transform on the live layers), source/drivers/gpud/src/backend/dc/{controller.rs,cursor.rs} (the pointer's plane: the mechanism)
---

## Context (cycle 51, read line by line)

- **Open is assembled from parts.** windowd puts a new window into the scene at `SURFACE_CREATE`
  and starts the enter spring at once (opacity 0→1, scale 0.92→1, ~350 ms) — on a surface the
  app has not painted. The content arrives at `seq=1` 560 ms (calculator) to 6.9 s (settings)
  later, the chrome's material layers (`apphost: submitted N layers`) after that. Three parts,
  three moments, and the animation is over before the first one.
- **Every animation tick re-draws the window.** A layer transform makes the window "drawn
  otherwise" for the planner: damage = old ∪ new extent (1.2 Mpx at 1440×853), the slot pass
  re-draws the wallpaper and every layer in it, and the window's own glass layers re-blur
  because their rectangle moves. At 60–80 ms a frame the spring gets four or five frames.
- **Drag is the worst window:** 12.8 passes a frame, 126 glass writes in 32 frames, between
  `press app0 zone=drag` and `wm release` at 200 pointer events a second.
- **The controller has the planes.** The stock tree shows 8 composer planes on the one CRTC
  (position, crop, z-order, ARGB), two in use (the frame, a 64×64 cursor). Our DC driver drives
  the same mechanism for the pointer (`cursor_layer_on`, `cursor_move`).
- The reference platforms: the window manager shows a *starting window* (colour + icon) at
  launch and swaps in the app on its first frame; an animated node is cached as one texture;
  a dragged window rides a hardware composer plane; blur is static unless its backdrop changes.

## Goal

A window opens at the tap as one opaque surface and becomes the app in one frame; closes as
one snapshot while the app is already dead; moves on a display-controller plane with zero GPU
pixels; its glass blurs once when it comes to rest. `gpud: ux frame ok` during open, close and
drag; `windowd: ux input ok` during drag.

## Non-Goals

- The app's first frame time itself (6c; the starting layer hides it, the stage times name it).
- Window chrome and materials (TASK-0337). The compositor's delivery contract (ADR-0075).

## Three lenses

- **Scope**: planner `nexus_gfx::frame` (a snapshot layer source, the "no write while the
  transform is active" rule, the glass-write cause counter), gpud (the snapshot blit, a plane
  bind for a window VMO, the cause line), windowd (`transitions.rs` replaced: the starting
  layer, the snapshot handoff, the plane during drag; `loop_telemetry.rs` reports every window
  with traffic), app-host (`ux launch` carries its stages). Out: the apps, window-kit, the kernel.
- **Invariant**: the screen never shows a stale or torn window — the swap starting layer → app
  and snapshot → gone happen in one frame each (`test_reject_a_swap_that_shows_both`); a plane
  move is latched once per vblank; a snapshot is freed with its window (6a's rule: it dies with
  its last capability).
- **Contract**: RFC-0101 §8 extension — the snapshot source and the plane bind; the launch
  intent from abilitymgr to windowd (the starting layer before the process exists).

## Plan (each step read by `gpud: ux frame`, `gpu work`, the cause line)

- **P0 — the causes and the stages.** `gpud: glass writes (backdrop= moved= new= full=)`: why
  each glass was written in the window (the planner names the cause per write, host-tested).
  `windowd: loop` reports every one-second window with ≥ 1 present (today ≥ 8, which hid the
  board). `apphost: ux launch` carries its stages (`mount_ms= layout_ms= text_ms= render_ms=
  present_ms=`). Board: the plane capabilities measured read-only on the stock tree (scaling,
  per-plane alpha, formats) — decides what the plane animates (position only, or scale too).
- **P1 — the starting layer.** At `abilitymgr: launch` windowd mounts one opaque rounded layer
  (the manifest's colour, the baked icon) at the window's frame and runs the enter spring on
  it (one layer, the opaque program, no glass write: damage per tick = its rectangle). At the
  first present with the app's layers delivered (`app layers=N`) the app replaces it in one
  frame (a ≤ 100 ms cross-fade if the design wants one). Host: the swap's frame.
- **P2 — the snapshot.** gpud copies the window's rectangle out of the slot into a snapshot
  texture (one blit); the app's layers leave the scene at once, the app may die (6a); the exit
  spring runs on the snapshot. Minimize and restore the same way. Host: the planner's snapshot
  layer; its golden equals the live window's pixels.
- **P3 — blur at rest.** No glass write while a layer's transform is active or a drag holds
  it; one write on rest (the key's change collected, not acted on). Host: the rule, the
  write-cause counts.
- **P4 — the dragged window on a plane.** On `press … zone=drag` the snapshot (an ARGB DMA
  VMO) binds to a free composer plane at the window's position; pointer events move the
  plane (bus writes, latched per vblank); on release the window returns to the scene at its new
  place and blurs once. The scene underneath is not redrawn during the drag. QEMU: the same
  path through virgl's composite (no plane; the snapshot layer drawn). Board: zero GPU pixels
  during the drag (`gpu work` slot_px ≈ 0 in a drag window).
- **Proof**: host tests per step; `just test-os visible`; board cycles with the operator's
  series; the package closes when a drag window and an open/close window read `ux frame ok`.
