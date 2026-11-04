---
schema: 1
id: TASK-0354
title: "Compositor motion: a transition stated once, moved by windowd for every shown frame; springs in closed form that keep their velocity"
status: in_progress
owner: ["@ui"]
created: 2026-11-03
updated: 2026-11-04
subsystem: ui
components: [windowd, app-host, animation]
kind: ui
novelty: variant
estimate: 3
started: 2026-11-03
dates_source: recorded
goal:
  metric: "app-host wake-ups per frame of a Tier-2 transition; frames dropped per transition (32-frame window); closed-form spring deviation from RK4"
  baseline: "board 2026-11-03 (board--2026-11-03T19-13-30): 10.3 wake-ups per sent frame, 77 ms median from pulse to the sent frame (114 ms weighted), 24 of 32 frames dropped (median window); RK4 fixed-step springs restart on a retarget"
  target: "0 app-host wake-ups between a transition's start and end; 0 of 32 frames dropped per transition on the board; closed-form within 0.5 px of RK4; velocity continuous on a retarget"
  instrument: "apphost: hops motion (pulse>sent= ticks=) · windowd frames(moved=) line · gpud: ux frame line · host tests"
consumer: TASK-0353
links:
  - label: Model
    target: "docs/dev/ui/foundations/animation.md (Tier 1/Tier 2; 'motion as layer properties is the follow-on')"
  - label: "Shown time"
    target: "TASK-0343 / RFC-0110 (the predicted landing the motion is computed for)"
  - label: Budget
    target: "RFC-0108 (frame line)"
phases:
  - id: P0
    story: "a spring is solved in closed form at any time, and a retarget keeps its velocity"
    state: done
    gate: "host: max deviation closed-form vs RK4 <= 0.5 px for every token; velocity jump on retarget = 0"
    started: 2026-11-04
    done: 2026-11-04
    evidence: "cargo test -p animation --test spring_closed_form_tests (7/7, 2026-11-04)"
    note: "2026-11-04 built: animation::Spring (spring.rs) closed form under/critical/overdamped in f64 via libm; retarget keeps (x, v); settled = SpringSim thresholds. Host spring_closed_form_tests 7/7: compositor dampings (default 100/10, enter 260/24, exit 380/34, critical 200, overdamped 100/40) within 0.5 px of RK4 at 60 Hz; every damping incl. reduced 1000/100 within 0.05 px of RK4 at 1 kHz (the 60 Hz RK4 is 2.7 px off for the stiff spring: its error, not the closed form's); velocity jump on retarget = 0 exactly (the restarted RK4 jumps). The tokens' own springs (response, damping) are P3; P0 proves the solver over every damping regime."
  - id: P1
    story: "the cost of today's motion is measured on the board: app-host wake-ups and CPU milliseconds per transition frame, frames dropped per shell transition"
    state: done
    gate: "board cycle (shared with TASK-0351 P0 and TASK-0355 P0): wake-ups per frame, CPU ms per transition frame and dropped frames of 32 logged for panel, launcher and window open/close"
    started: 2026-11-03
    done: 2026-11-03
    evidence: build/logs/board--2026-11-03T19-13-30
  - id: P2
    story: "an app states a value change or transition once on a layer, and windowd moves the layer for the time each frame is shown"
    state: blocked
    gate: "board: apphost wake-ups during a transition = 0; app CPU raster per transition frame = 0; host test_reject_a_motion_on_a_layer_the_sender_does_not_own, test_reject_an_unknown_motion_token"
    note: "2026-11-04: opens with the RFC-0109 amendment (a declared layer carries a motion record) once TASK-0351's board cycle has judged P5/P10 — the operator's order: the frame budget first, then motion on it"
  - id: P3
    story: "the motion tokens are springs (response, damping), and reduced motion still resolves to the end state"
    state: planned
    gate: "host goldens sampled at named times for all 9 tokens; every token a spring settling within 550 ms, the default critically damped, micro-motion under 200 ms, panels and launcher 250-400 ms (latency-reference.md); reduced motion = 0 frames of motion"
  - id: P4
    story: "every shell transition holds the frame on the board"
    state: planned
    gate: "board: panel, launcher and window open/close each 0 of 32 frames dropped, gpud: ux frame ok"
---

## The ask

Operator, 2026-11-03: the animation model as smooth as the reference class (iOS: the render server interpolates, the app is out of the frame loop) and efficient on the BPI-F3's small cores.

## Measured before the cut

- Tier 1 (window transform) already runs in windowd/GPU without a re-raster (TASK-0336, TASK-0338).
- Tier 2 (in-surface `.animate`/`.transition`/`.effect`) ticks the app-host on every frame pulse and re-rasters on the CPU (`scene_raster/src/anim.rs`).
- P1, board 2026-11-03 (`board--2026-11-03T19-13-30`, `docs/board/measurements/2026-11-03-motion-input-frame-baseline/`):
  52 motion episodes, 4 776 wake-ups for 462 sent frames — 10.3 wake-ups per frame; a transition
  frame takes 77 ms from the pulse to its send (median of the episode means; weighted 114 ms, worst
  6.4 s); 42 of 52 episodes woke more often than they presented; frames dropped 24 of 32 (median
  window) while anything moved. A Tier-2 transition is a 10-fps thing on the app side. The lines do
  not name which transition an episode was (panel, launcher, window): that attribution comes with
  windowd's motion record in P2, which knows the layer.
- The reference numbers (`docs/dev/ui/foundations/quality/latency-reference.md`, 2026-11-03): the
  reference laptop's panel is 60 Hz, so the unit of smoothness is 0 of 32 frames dropped at 16.7 ms;
  transitions are 200–400 ms springs, critically damped by default; micro-motion under 200 ms.

## Decision (2026-11-03, operator: "everything smooth — advise")

Everything that moves a large area runs on the compositor: the shell's panels and the launcher
(their open, close and paging), the windows (already Tier 1), the scroll layers (RFC-0109). The app
states the transition once and is out of the frame loop. In-surface micro-motion (a press, a wash, a
list item's fade) stays Tier 2 on the app's CPU: a DSL node is not a layer, and a layer per animated
node would break the per-surface layer budget. Micro-motion is small by the reference numbers (under
200 ms, a few hundred pixels), so its cost is bounded by a gate instead: CPU ms per micro-motion
frame measured in P1, damage limited to the node's rect. Smoothness on the reference device is
therefore: large motion on windowd for the shown time, small motion cheap and short.

## Three lenses (architecture-review, 2026-11-03)

- **Scope** — `userspace/ui/animation` (the spring solved in closed form at any time, retargeted with
  its velocity; one physics SSOT for both drivers); the surface-layer wire (`source/libs/nexus-wire`,
  the display protocol: a motion record on a layer — property, target, token, start velocity, as a
  bounded fixed frame); windowd (`compositor/runtime/transitions.rs` and the layer composer: the
  motion of every layer evaluated for the predicted shown time, RFC-0110); app-host (`probe/anim.rs`:
  states the transition once for a layer-bound node, keeps ticking only for Tier-2 nodes); the DSL
  `.transition` lowering for layer-bound nodes; the shell's panels and the launcher declared as
  layers. Out: in-surface micro-motion as layers (see Decision); the GPU's per-pixel cost
  (TASK-0351); the landing prediction's accuracy (TASK-0343); the tokens' design values (TASK-0353).
- **Invariant** — windowd moves only a layer the sender owns (identity = the kernel sender id), with a
  token from the curated set, a target inside the layer's range and a bounded duration; the motion is
  computed for the shown time and resolves to the stated end state under reduced motion; the app is
  not woken between a transition's start and end. Negative tests:
  `test_reject_a_motion_on_a_layer_the_sender_does_not_own`, `test_reject_an_unknown_motion_token`,
  `test_reject_a_motion_target_outside_the_layer_s_range`; host: closed form within 0.5 px of RK4
  for every token, velocity continuous on a retarget.
- **Contract** — RFC-0109 amended (a declared layer carries a motion record) and RFC-0110 extended
  (the per-layer motion evaluated at the shown time); an ADR seed "windowd moves the declared layers"
  extending ADR-0076 (the app decides what moves, the compositor moves it); the animation crate stays
  the physics SSOT of ADR-0031. The wire change is why P2 opens with the RFC amendment, not code.

## Not doing here

- The GPU's per-pixel cost (TASK-0351), the landing prediction's accuracy (TASK-0343), micro-motion
  as layers (a layer budget concept first, if ever), the design values of the tokens (TASK-0353).

## Definition of Done
