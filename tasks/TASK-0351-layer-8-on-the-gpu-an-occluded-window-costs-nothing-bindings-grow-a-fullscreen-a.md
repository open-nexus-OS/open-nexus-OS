---
schema: 1
id: TASK-0351
title: "Layer 8 on the GPU: an occluded window costs nothing, bindings grow, a fullscreen app is scanned out directly and apps render with the GPU"
status: in_progress
owner: ["@ui", "@gpu"]
created: 2026-11-03
updated: 2026-11-04
subsystem: ui
components: [gpud, windowd, nexus-gfx, app-host]
kind: service
novelty: first
estimate: 3
started: 2026-11-03
dates_source: recorded
goal:
  metric: "windows one frame can hold; GPU bindings per board context; apps that render with the GPU; GPU ns per textured pixel; frame p99 and drops on the board"
  baseline: "64 layers per frame (topmost 62 windows, occluded bodies still drawn); 128 bindings per board GPU context; 0 GPU-rendered apps; no direct scanout (TASK-0348 'Not doing here'); 13 ns/px median, 25 ms GPU frame median (cycle 61) against the stock compositor's 1.9-3.7 ns/px"
  target: "occluded windows cost 0 draws; bindings grow past 128; a fullscreen app scanned out without a compose; one app renders with the GPU; <= 4 ns/px median; frame p99 <= 16.7 ms with glass and 0 of 32 frames dropped during a transition"
  instrument: "gpud: gpu gate scene ok · gpud: gpu work (ns_per_px=) · windowd frame line (drawn=, occluded=) · ux frame p99 line"
consumer: TASK-0336
depends_on: [TASK-0348]
links:
  - label: "Scope from"
    target: "TASK-0348 'Not doing here' (the old number TASK-0350 there means this task)"
  - label: "Programs from"
    target: "TASK-0335 P4's rest (the blur's dual filter, the SDF at the edges only, the shadow as a ring), parked to layer 8 by the operator 2026-10-22"
phases:
  - id: P0
    story: "the frame cost with 12 and 30 windows and the binding use are measured on the board"
    state: proving
    gate: "board cycle: frame p99, ns_per_px, layers per frame and bindings in use logged for 12 and 30 windows"
    started: 2026-11-03
    note: "2026-11-04 cycle 12-28-47: at most 4 windows live at once (windows push n<=4, layers<=21); the 12- and 30-window series is still owed — the next cycle opens the cascade twice."
  - id: P1
    story: "a window hidden behind others costs the frame nothing"
    state: proving
    gate: "host: the lane's cascade under a filled window leaves its middle row (4 of 11) out (culling_model); QEMU many-windows: fill and return chords applied, the pixel proof unchanged (the CPU path blends glass and culls nothing); board: windowd: windows occluded (n= on the operator's stack"
    started: 2026-11-04
    evidence: build/logs/many-windows--2026-11-04T09-45-42
    note: "2026-11-04 cycle: fill chord over 3 cascade windows, windows(occluded=0) throughout — the top row's shadows reach the top bar outside the filled body, as the host model says (4 of 11 are the middle row); the board proof needs a cascade of 8 or more."
  - id: P2
    story: "a board GPU context binds as many surfaces as the windows need"
    state: proving
    gate: "host: a space binds 130 bindings past a reserve of 2 (nexus-driverkit), the surface window reaches the layout's GiB; board: 30 windows with 0 binding failures, gpud: bindings of= grows past 128"
    started: 2026-11-04
    evidence: build/logs/many-windows--2026-11-04T09-45-42
    note: "2026-11-04 cycle: bindings in_use<=48 of=128 contexts=1, no binding failure with 4 windows; the growth past 128 needs the 30-window scene (P0)."
  - id: P3
    story: "the frame the planner scans out directly is named: an opaque, untransformed, plain window body from a surface of its own covering the display whole (RFC-0101 §8 amended) — the contract and the host-tested decision; the contiguous surface and the flip are P9"
    state: done
    gate: "host: tests/frame_scanout.rs — the fullscreen body is named, nine lesser bodies, a layer above and a body in motion are refused; RFC-0101 §8 carries the contract"
    started: 2026-11-04
    done: 2026-11-04
    evidence: "userspace/nexus-gfx/tests/frame_scanout.rs (4 tests green 2026-11-04); docs/rfcs/RFC-0101 §8 direct scanout"
  - id: P4
    story: "an app may open a GPU context of its own — normal priority, its manifest's capability, its own surfaces, charged to its footprint (RFC-0101 §1 amended, ADR-0095) — the contract; the app-host GPU renderer is its consumer task"
    state: done
    gate: "RFC-0101 §1 and ADR-0095 name the authority, the memory charge and the death path; gpud's refusal of every sender but windowd still holds (test_reject_open_from_another_service)"
    started: 2026-11-04
    done: 2026-11-04
    evidence: "docs/adr/0095 (Proposed 2026-11-04); docs/rfcs/RFC-0101 §1; the consumer task minted below"
  - id: P5
    story: "a frame costs what the hardware asks, not what our path adds: the frame's fixed cost and the pass's area measured through our path against the stock sweep (gpud: gpu pass sweep), then cut — the passes a frame runs and what each pass costs beyond its pixels"
    state: proving
    gate: "board: gpu pass sweep q16_us within 2x of the stock's 0.64 ms and area16_us within 2x of q16_us (the area reaches the hardware); gpud: gpu gate scene ok (hover below 8.3 ms, cold below 23.3 ms)"
    started: 2026-11-04
    note: "Cycle 12-28-47 read against the kernel: a pass's fixed 5.8-13 ms is the wake-to-run latency (irq/timer deliver = enqueue only, S_EXT returns without a switch, rotation on the 10 ms tick, every service Normal; the trace's 1 ms slices woke 0.3-5.1 ms late; bring-up passes 0.5-0.8 ms on an idle CPU). ADR-0096 built: Scheduler::ready_above_current + preempt at S_EXT/S_TIMER/ecall exit; init raises the soft-realtime chain (service_topology::soft_realtime) to Interactive; gpud trace line late_us=. Criterion for the next cycle: late_us ~0, q16_us < 2x the area rung (1.7 ms), hover_us < 8300. Parked with numbers: emit ~90 us a draw on the CPU (H10); HSR for opaque windows (opaque_px+straight_px vs slot_px, ~2 % overdraw at 4 windows); FP16 never emitted; FBCDC off; TQ unused in the desktop; sched host tests are target-only (pre-existing gap)."
  - id: P6
    story: "the passes a frame runs hold the budget on the board, with glass and while something moves"
    state: planned
    gate: "board: gpud: gpu gate scene ok, frame p99 <= 16.7 ms with glass on the operator series, 0 of 32 frames dropped during a transition"
  - id: P7
    story: "every effect's mathematics lives in nexus-gfx's kernel tables and the executors interpret it (ADR-0094): the blur taps shared by the shaders and the raster, the dual filter harvested from nexus-effects into the table, the crate and the dead crates deleted, no tap weight or coverage formula left in a driver or a shader source"
    state: done
    gate: "grep: no kernel literal under source/drivers/gpud or resources/gpu/shaders outside the table's uniforms; cargo tree: nexus-effects gone; ADR-0094 Accepted"
    started: 2026-11-04
    done: 2026-11-04
    evidence: "build/logs/board--2026-11-04T12-28-47 (gpud: gpu blur radii ok r8=4 r30=1 r40=1 tolerance=10/13/13 under the oracle); build/logs/visible--2026-11-04T11-15-47 (gpu blur golden ok); nexus-effects gone; ADR-0094 Accepted"
  - id: P8
    story: "a board cycle's gate values form a series: every gate line carries the last cycle's value and a regression past the margin is FAIL naming that cycle (ADR-0094)"
    state: done
    gate: "board: gpud: gpu gate scene and ux frame lines carry last=; docs/board/measurements series table generated by nx ledger; a 10 % regression in a cycle prints FAIL (cycle=)"
    started: 2026-11-04
    done: 2026-11-04
    evidence: "build/logs/board--2026-11-04T12-28-47: last= on gpu gate scene (last=48505/35803/12739 cycle=2026-11-03T19-13-30), gpu work (last=6), ux frame (last=174889), gpu pass sweep (last=0/0); the cycle recorded as 2026-11-04T12-28-47 in series.toml. Found: the gate line crossed the console's 256 bytes — shortened."
  - id: P9
    story: "a fullscreen opaque app goes to the display plane: windowd allocates a contiguous scanout-class surface for a window entering fullscreen, gpud flips it instead of a slot of the ring when the planner names it, the compositor's passes are 0 while it presents"
    state: planned
    gate: "board: gpud: gpu direct scanout ok (frames= passes=0) while a fullscreen app presents; test_reject_a_flip_of_a_paged_surface"
  - id: P10
    story: "the emit costs what its draws write: a pass's draws are walked once per frame (the plan's wallpaper pieces and each layer's draws in order) and the emitter indexes a kept list instead of asking the plan per draw"
    state: proving
    gate: "host: the walked draws equal the indexed walk for the cascade, the glass and the twelve-window scenes (frame_board), and the walk touches each layer once; board: gpud: gpu frame us split emit per draw <= 15 us at 12 windows"
    started: 2026-11-04
    evidence: build/logs/headless--2026-11-04T13-54-39
    note: "2026-11-04 built: Board::walk (frame/walk.rs) + gpud Kept list; host frame_board_walk: 12 windows 178 draws, walk 4.9 us/draw vs 73 us/draw asked per draw (debug build, x86) — parity for window, glass-over-cascade, twelve windows. P11 (passes pipelined) needed no work: desktop.rs frame_of already pipelined=true. Kernel P5 files split for the LOC ratchet: sched/class.rs, trap/pending.rs deliver_external + stage_wake (handler.rs 1106->1084, sched/mod.rs 634->608). Board gate: gpu frame us split emit / draws <= 15 us."
---

## The ask

## Measured before the cut

- Cycle 61 (2026-10-22): 13 ns/px median, 25 ms GPU frame median; the stock compositor 1.9–3.7 ns/px.
- P0, board 2026-11-03 (`board--2026-11-03T19-13-30`, `docs/board/measurements/2026-11-03-motion-input-frame-baseline/`),
  **4 windows at most** (the 12- and 30-window series is still owed — it opens package 3):
  `ux frame` 26 FAIL / 1 warn of 27 windows, dropped 24 of 32 median, worst frame 102 ms median;
  `gpu work` 6 ns/px median over 2.8 Mpx slot area median, `layers` median 16, max 19 of 64;
  `bindings` in_use max 49 of 128 in 1 context; `windowd: hops frame` `sent>acked` 90 ms mean.
  The tap's dominant hop (TASK-0355 P0) is this frame.
- **P5, read 2026-11-04 from the same log — the frame's cost follows its passes, not its pixels.**
  `gpu frame us` windows: wait 17.0–28.7 ms a frame at 2.2–4.8 passes = 6.0–9.1 ms a pass over
  0.98–1.78 Mpx of area; the first frame (one opaque full-screen pass, 2.07 Mpx) 6.85 ms; the
  gate scene's hover frame (ONE pass over a 1x1 damage) 12.7–13.5 ms in every cycle of the day,
  its cold frame 46–48 ms serialized / 34–36 ms pipelined (`FAIL gpu gate scene` since cycle 61).
  `blur_px` is 0.1–3 % of a frame's pixels. The stock stack's sweep (2026-10-10): a 16x16 frame
  0.64 ms, the full textured quad 4.43 ms, blended 7.17 ms, 1.8 ns a pixel. **Our fixed cost a
  pass is some 20 times the stock's; the dual filter would cut nothing.** P5 is re-aimed at the
  fixed cost and at whether a pass's area reaches the hardware (`gpud: gpu pass sweep`, the stock
  sweep through our path, at boot after the gate scene). Hypotheses for the next cycle: H1 the
  pass's area is not honoured by the hardware (`area16_us` ≈ `full_us`, not ≈ `q16_us`); H2 a
  fixed cost of several ms a pass in our submit/wait path (`q16_us` ≫ 0.64 ms while the slope
  `full_us − q16_us` ≈ the stock's 3.8 ms); H3 the wait loop's period adds up to 1.4 ms
  (`pass_wait_us` iters). Decision: whichever holds names P5's first cut.

## Three lenses (architecture-review, 2026-11-04)

- **Scope** — P1: windowd's scene builder (`compositor/runtime/scene.rs`, a new `occlusion.rs`)
  leaves a window out of the frame whose body and shadow lie wholly under the union of the opaque
  rectangles of the windows above it (a solid body, a glass body whose blurred back is drawn, an
  opaque declared region, an opaque scroll band — each at identity transform); the coverage test
  is nexus-gfx's (`frame/occlude.rs::covered`, host-tested); the loop line gains
  `windows(drawn= occluded=)` and a high-water line `windowd: windows occluded (n=`; the
  many-windows lane grows to 30 windows. P2: the binding table of a board GPU context
  (`nexus-driverkit::gpu::VmSpace`) grows from the heap like every table of ADR-0093 — its reserve
  is the old constant, `Full` only when memory or the address range ends; the app-surface VA window
  (`gpud/surface_va.rs`) widens with it. P5: the blur as a dual filter (down/up chain,
  `resources/gpu/shaders/blur.wgsl`, `frame/board.rs` pipes, `desktop.rs` pipelines) — the SDF at
  the edges and the shadow ring are TASK-0335 P4's and done. P3 (direct scanout) and P4 (an app's
  GPU context) each open with their RFC amendment (RFC-0101 §scanout, §7b) before code. Out: the
  gpud planner's single-occluder cut (correct, stays), the compositor's motion (TASK-0354), the
  panels' pixels (TASK-0312/0313), gpud's restart.
- **Invariant** — a window left out is one no pixel of which the frame would show: coverage is exact
  (the union, never the largest occluder; a translucent, transformed or resizing body occludes
  nothing and is never left out); a culled window keeps its transform, its input and its surface
  (it is not hidden, only not drawn); `windows drawn` counts the bodies the frame draws. Negative
  tests: `test_reject_a_translucent_body_as_an_occluder`, `test_reject_coverage_with_a_gap`
  (host, nexus-gfx); the many-windows lane's pixel proof unchanged at 12 windows. P2: a binding
  past the reserve is accepted, `test_reject_a_binding_past_the_address_range` stays; the board
  binds 30 windows with 0 refusals.
- **Contract** — RFC-0101 §8 (the planner's occlusion, extended to the compositor's culling: the
  same `opaque_rect` rule), RFC-0109 (the layer budget: a culled window spends none), ADR-0093 (the
  table rule for P2), RFC-0108 §hops (the lines). P3/P4: RFC-0101 amendments, an ADR for an app's
  GPU context (identity = kernel sender id, its memory charged to it).

## Three lenses, amended 2026-11-04 (ADR-0094: one kernel, three executors)

The operator's finding while P1 was built: the blur's dual filter P5 was about to derive for the
shader exists, finished, in the dark crate `userspace/ui/effects` (nexus-effects); the raster
(`nexus-gfx/src/raster`) is a hand-written mirror of the shaders and the board's blur oracle, tied
to them by nothing; the gates have a bar but no memory. ADR-0094 decides: an effect's mathematics
is a table in nexus-gfx, every executor interprets it, the raster is the oracle, a gate's value is a
series. The stack's direction stands behind it — what a frame is lives in nexus-gfx, a driver is a
thin executor, so the next driver is a new executor, not a new renderer.

- **Scope** — P5: `nexus-gfx/src/frame/kernels.rs` (new): the blur kernel per radius (scale, tap
  offsets, weights — today computed twice: `blur.wgsl` `2^(k·d²)` and `raster/blur.rs` `expf`),
  then the dual filter's chain harvested from nexus-effects' `dual_kawase_blur`; `tools/nxgpu`
  `overrides.rs` emits the shader's constants from the table; `raster/blur.rs` reads the table
  (`MAX` radius 20 today against the planner's 40 — a drift the table ends); a golden lane
  compares the virgl readback with the raster per radius. P7: nexus-effects' remaining knowledge
  (the nine-slice shadow) checked against the shadow ring, then the crate and its two test
  harnesses (`tests/ui_v4_host`, `tests/ui_host_snap`) deleted with the dead crates the audit
  found (nexus-msdf, ui_renderer, nexus-shape, nexus-image, nexus-cursor) — root `Cargo.toml`
  members, the unused `nexus-effects`/`nexus-sdf` dependencies of windowd and `nexus-sdf` of gpud.
  P8: a `last=` field on the gate lines (`desktop_gate.rs`, the ux frame line, RFC-0108 §hops)
  fed from the last recorded cycle's table under `docs/board/measurements/`, generated by
  `nx ledger`. Out: the planner's pass structure, the DSL's own raster, the AM-protocol SSOT and
  the virgl per-source upload (own tasks after package 3).
- **Invariant** — no tap weight, radius table or coverage formula outside nexus-gfx's table (P7's
  grep gate); the raster is a product path and the oracle — a cut whose readback leaves the
  tolerance is red on the host before the board; a gate's regression names its cycle. Negative
  tests: `test_reject_a_kernel_whose_weights_do_not_sum_to_one`, `test_reject_a_readback_past_the_tolerance`.
- **Contract** — ADR-0094 (Proposed; Accepted with P5's readback gate), RFC-0101 §8 (gains the
  kernel table), RFC-0108 §hops (gains `last=`), ADR-0093 (P2, done 2026-11-04 on the host).

P1's lane finding (2026-11-04, `many-windows--2026-11-04T08-53-53`): the headless lane composes
on the CPU, where a glass body is blended and hides nothing — the planner's rule counts only a
GPU-drawn glass back (`back`) — so `windows occluded` is the GPU path's line (virgl, the board);
the lane keeps the fill and return chords, the host model pins the rule.

## Not doing here

## Definition of Done
