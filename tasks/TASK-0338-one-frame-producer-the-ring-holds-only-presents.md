---
title: TASK-0338 One frame producer — the display ring holds only presents; a window's move, a transition tick and a scroll ride in windowd's present, inside its budget
status: Done (2026-10-21, board cycle 54: 0 present NACKs; committed in 984a977c)
owner: @ui @gpu
created: 2026-10-21
depends-on: [TASK-0336 (the transform carried in every present), TASK-0335 (the planner's damage)]
follow-up-tasks: [the non-parking compositor (ADR-0075), TASK-0336 P4 (the dragged window on a plane)]
links:
  - RFC: docs/rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md (§8: the ring, the present's ack as the frame clock)
  - RFC: docs/rfcs/RFC-0108-ux-latency-contract-instant-and-smooth-as-numbers.md (the budgets this unblocks)
  - Measurement: docs/board/measurements/2026-10-21-gpu-vendor-per-pixel/README.md ("Cycle 53": the ring overflow)
  - Code: source/services/windowd/src/compositor/runtime/{present.rs,transitions.rs,intent.rs,input_scroll.rs,anim.rs,scene.rs}, source/drivers/gpud/src/{service.rs,present_chain.rs,backend/dc/present.rs}, userspace/nexus-gfx/src/frame/layers.rs
---

## Context (cycle 53, read from the log)

- Launches were fast (calculator 140 ms, chat ~0.6 s), frames were not: `gpud: ux frame FAIL`
  (15–32 of 32 dropped), `windowd: ux input FAIL` up to 5.7 s, windowd presenting 1–7 times a
  second while damage queued up to 19.6 Mpx/s.
- All 21 present NACKs (`status=0x02`) were `gpud: chain reason: resource exhausted`: the
  display ring had no free plane. The ring has three planes — scanned, queued for the blank,
  waiting — and windowd's two presents in flight fill it exactly.
- windowd also sent `OP_SET_LAYER_TRANSFORM` fire-and-forget on every spring tick of an open or
  close and on every drag step, and `OP_SET_LAYER_SCROLL` on every scroll step; gpud drew each
  into the ring as a frame of its own (`present_chain::recompose`), outside windowd's budget.
  The next present found no plane, was NACKed, and windowd requeued **full-screen** damage with a
  full recompose — the loop slowed, more ticks piled up. The bursts sat at chat's open and close.
- Every transition tick also fell into the animation tick's legacy `panel_dirty` arm and queued
  an 826×260 px desktop rectangle as damage (a proof panel that no longer exists), under the
  windows' glass.

## Goal

windowd is the only producer of frames: every frame the display shows is a present windowd
sent, inside its budget of two in flight, coalesced to the display's frame clock (the acks at
the blank). A window's transform, a transition tick, a drag step and a scroll step owe a frame;
the next present carries the latest state. gpud draws nothing of its own.

## Non-Goals

- The dragged window on a controller plane (TASK-0336 P4).
- The hover wash as a layer (TASK-0337 P3).
- windowd never parking on an app (ADR-0075, layer 6d).

## Three lenses

- **Architect:** extends RFC-0101 §8 (presents are the ring's frames, the ack is the frame clock)
  and TASK-0336's `Command::LayerTransform` in every present. Retires wire ops 6
  (`OP_SET_LAYER_SCROLL`) and 10 (`OP_SET_LAYER_TRANSFORM`) and the layer's `scroll_id` word;
  adds one wire meaning: `Command::Damage` with `TileRect::NONE` names no damage.
- **Security:** the wire shrinks; the one new accepted value is exact (`TileRect::NONE` from
  origin 0 only) — `test_reject_damage_empty_or_wrapping` refuses every other empty rectangle
  and the empty one claimed by a window. A full ring is a broken budget and is refused by name
  (`gpud: FAIL dc present (no free plane: …)`, forbidden on the board ladder).
- **Pragmatist:** no new op, no new state machine — the owed-frame flag beside the pending
  rectangles, the planner's existing per-layer damage. Not doing: planes, hover, ADR-0075.

## Plan / what was built (2026-10-21)

- **windowd:** `owe_frame()` (a frame owed for what moved alone) replaces every
  `OP_SET_LAYER_TRANSFORM`/`OP_SET_LAYER_SCROLL` send — transitions, drag steps, closes, scroll
  steps and flings, the frame clock's keep-alive (it queued a pointer-rect present before). The
  flush builds a present when only a frame is owed; with no rectangle it names
  `TileRect::NONE` (a present with layers and no `Damage` is damage everywhere). Every present
  carries every slot's transform — the identity for a free slot, so a slot's next tenant never
  inherits a fade (no op retires it any more). The transform holds (`transform_held`) are gone:
  there is no op for them to hold back. A window-transform tick no longer queues the legacy
  panel rectangle. The loop line counts the new kind: `frames(… moved= …)`.
- **gpud:** the two op arms, the deferred recompose (`scroll_flush_pending`,
  `present_chain::recompose`), the override setters on both executors (`Display` and
  `FrameGpu`: `set_layer_scroll`, `set_layer_transform`, `flush_layer_overrides`; virgl's
  `record_layer_*`/`flush_layer_scroll` and their one-shot lines) are deleted. A present that
  finds no free plane is refused by name instead of as `resource exhausted (bump heap?)`.
- **nexus-gfx:** the planner's scroll override (`set_scroll`, the per-id rows) and the
  `CompositeLayer`'s `scroll_id` word (19 words now) are deleted — a scroll is a present whose
  body layer carries its new `src_row_abs`; the scroll band stays (a backend that uploads the
  atlas uploads it once). `TileRect::NONE`, its validation, `try_damage_none()`.
- **nexus-display-proto:** ops 6 and 10 retired, the numbers reserved (an old sender gets
  `STATUS_MALFORMED`).

## Proof

- Host: `frame_motion` — `a_present_that_names_no_damage_redraws_only_what_moved` (old ∪ new
  extent, nothing for an unchanged frame, never the screen),
  `a_scrolled_body_is_redrawn_alone_and_its_glass_keeps_its_blur`; `frame_plan` — a new set
  brings the scrolled row and keeps the transforms; wire — `no_damage_roundtrips_over_the_wire`,
  `test_reject_damage_empty_or_wrapping`, `scroll_band_roundtrips_over_the_wire`.
- QEMU `headless`, `visible`: green on the ladder (transitions and scroll now presents).
- Board cycle 54: no `windowd: gpud present nack`, no `gpud: FAIL dc present (`; the loop line's
  `present=` against `apply=`, `moved=`; `gpud: ux frame`, `windowd: ux input` against cycle 53.

## Results

- 2026-10-21: built; host tests, `just check`, `just diag`, `build-os-workspace` (0 warnings),
  QEMU `headless` and `visible` green (no NACK; the automated visible lane opens no app window —
  transitions, drags and scroll are proven on the board).
- **Cycle 54 (board, 2026-10-21):** the operator: "slowly usable — not instant yet, but usable".
  Present NACKs 21 → **0**, no `gpud: FAIL dc present (`; the worst input wait per 16 frames:
  median 1 324 → 833 ms, max 5 697 → 1 531 ms. Left, and not this task's: the GPU frame
  (22–59 ms average, ~78 % of frames over 16.7 ms — the frame-cost lane) and windowd's own wait
  before a submit at app launches (to be measured: the per-row atlas copy of an app present, the
  retained-plane rewrite, a park on an app queue). Ladder complete (76 rungs); acks desktop,
  pointer, gpu-desktop (the rest not checked this round).
