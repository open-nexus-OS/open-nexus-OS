---
title: TASK-0336 A window opens, closes and moves as ONE surface — the starting layer at the tap, the snapshot on close and drag, the dragged window on a display-controller plane, glass blurred at rest
status: In Progress (P0–P3 built 2026-10-21, board cycles 52–53 read; the frame throttle cycle 53 found is TASK-0338's; P4 at its measurement)
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

## Results

- **Built 2026-10-21 (host-green, board cycle 52 pending)** — the window as one surface:
  - **P0 the causes and the stages.** The planner names why each glass is written
    (`frame::glass::WriteCause`: band, backdrop, moved, new, full; `Planned::writes`), gpud
    prints `gpud: glass writes (band= backdrop= moved= new= full=)` per 32-frame window beside
    `gpu work`; `windowd: loop` reports every one-second window with a present (the ≥ 8 gate
    hid the board); `apphost: ux launch` carries `mount_ms= render_ms= create_ms=`.
  - **P1 the starting surface.** At the geometry handshake of a fresh floating window windowd
    reserves the slot, frames it and draws it as ONE solid surface (`solid_surface.rs`: a 16×16
    atlas patch of the window colour spread over the frame, `LAYER_SOLID | LAYER_OPAQUE`); the
    enter spring runs on it; at the app's `SURFACE_CREATE` the app's surface takes the window
    over in the same present (`end_starting_surface`). The starting slot is never handed to
    another launch (`free_app_index`), closes on a dead push (`note_app_channel_dead`) and on the
    launch deadline (15 s, the wait ring's clock). Every window asks its geometry now — `mode:
    auto` included (the WM's `AUTO_WINDOW` 320×270; app-host's probe size is gone) — so the
    calculator gets a starting surface too. The app's material regions go out before its first
    present (the first composite is the whole window, not its parts).
  - **P2 the closing surface.** The close button swaps the window to the same solid surface at
    once; the exit spring moves it; the slot closes at convergence. (The design asked for one
    surface without content both ways — the operator's model, 2026-10-21; no GPU snapshot
    texture is needed for it.)
  - **P3 blur at rest.** A glass of a window whose transform is not the identity — or touched
    only by such a window's damage — keeps its rows (`GlassCache::keep`,
    `RetainedLayers::in_motion`) and is written once at rest. A drag rides as the window's layer
    transform: no re-render per pointer move, the frame moves once at the release; every present
    carries the windows' transforms (`Command::LayerTransform`, RFC-0101 §8) — a release, a
    fullscreen toggle or a restore never shows the new frame with the old transform or the
    reverse. (The transform op between presents and its hold were retired by TASK-0338: gpud
    drew those as frames of its own and overflowed the ring — a drag step owes a present now.)
  - Host: `tests/frame_motion.rs` (causes, blur at rest, the transform in the present), the
    wire tests of `Command::LayerTransform` and the layer flags, `tests/frame_occlusion.rs`
    (the solid patch split into parts); windowd 193, nexus-gfx 153 + the frame suites.
- **P4 the dragged window on a composer plane — at its measurement (2026-10-21, read-only on
  the stock tree):** the DRM device exposes eight planes on the one CRTC (plane-0 the frame
  XR24 1920×1080, plane-1 the 64×64 AR24 cursor, plane-2..7 idle); the driver prints no plane
  capabilities (`dpu plane init ok`), and its debugfs holds no property listing. Next measure:
  the register sequence the stock driver writes when an overlay plane carries a buffer (a
  `kmssink` on plane-2), then the window plane as the pointer plane was brought up. **Trap:**
  reading the stock driver's debugfs `dump` file makes it write every framebuffer to `/tmp`
  (`spacemit_drm_dump_show`) — never read it (the two dumps it wrote were removed at once).
- **Cycle 52 (board, 2026-10-21 — `docs/board/measurements/2026-10-21-gpu-vendor-per-pixel/`
  "Cycle 52"):** the operator: "much, much better — still far from instant". Frames 19–37 ms
  (51: 33–81), passes 1.7–5.4 (3.4–12.8), layers a frame 3.7–5.8 (8.3); every launch shows its
  starting surface at its intent. Launches: calculator ≈ 0.4–0.46 s, the big windows 3–4 s —
  all of it the first frame's CPU raster (fixed in the rasterizer: interiors as spans, the ring
  where it is not zero; 4–10× on the host, byte-identical), the shell's window feed repainted
  the whole desktop (25 full-screen frames — now the rows its diff names), and `ux input`
  counted idleness (now the frames windowd owes). Cycle 53 reads all three.
- **Cycle 53 (board, 2026-10-21):** the operator: "better again". Launches to the first frame:
  calculator 140 ms, the shell 299 ms, chat 601–653 ms, settings 591 ms, stash 635 ms (cycle 52:
  3–4 s for the big windows). Frames still red (`gpud: ux frame FAIL`, 15–32 of 32 dropped;
  `windowd: ux input` up to 5.7 s): windowd presented 1–7 times a second. The cause: all 21
  present NACKs were a ring without a free plane — gpud drew every transition tick and drag step
  (`OP_SET_LAYER_TRANSFORM`) as a frame of its own beside windowd's two in flight; each NACK
  forced a full-screen recompose. Fixed by TASK-0338 (one frame producer), cycle 54.

