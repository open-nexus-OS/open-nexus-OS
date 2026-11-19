---
schema: 1
id: TASK-0354
title: "Compositor motion: a transition stated once, moved by windowd for every shown frame; springs in closed form that keep their velocity"
status: in_progress
owner: ["@ui"]
created: 2026-11-03
updated: 2026-11-19
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
    state: done
    gate: "board: apphost wake-ups during a transition = 0; app CPU raster per transition frame = 0; host test_reject_a_motion_on_a_layer_the_sender_does_not_own, test_reject_an_unknown_motion_token"
    started: 2026-11-04
    done: 2026-11-05
    evidence: build/logs/board--2026-11-05T13-12-18
    note: "panel, launcher and top-bar panel enters are motion layers rastered once (paint once, 0 per frame); no app episode carries a shell token; window sheets stay app-run (P7)"
  - id: P3
    story: "the motion tokens follow the design artifacts' curves exactly (the springs' overshoot included), growing entrances scale on the compositor about one origin, and reduced motion still resolves to the end state"
    state: done
    gate: "host goldens sampled at named times for every transition token against the artifacts' cubic-bezier (<0.005); a group scales about one origin in 1/10000 steps; board: panel, dialog and toast enters without an apphost hops motion episode"
    started: 2026-11-05
    done: 2026-11-05
    evidence: build/logs/board--2026-11-05T13-12-18
    note: "panel enters on the board without a token episode; dialog and toast enters on build/logs/usb-visible--2026-11-05T12-35-43 and in the operator's board test (the board trace ended at 300 KB, before them)"
  - id: P4
    story: "every shell transition holds the frame on the board"
    state: blocked
    gate: "board: panel, launcher and window open/close each 0 of 32 frames dropped, gpud: ux frame ok"
    note: "blocked on TASK-0351/P6: since P2 the frames a transition drops are the GPU passes' cost (board 2026-11-05T13-12-18: 3-12 of 32, max 19-37 ms), no app raster"
  - id: P5
    story: "opening a motion layer paints the layer alone: a relayout names the base's rows and the layers' rows apart, so a panel's open and a modal's scrim repaint no base row"
    state: done
    gate: "board: apphost motion layer paint_us the only raster of a panel open and of the power alert (no base rows under them in hops input); host what_a_motion_layer_holds_never_damages_the_base"
    started: 2026-11-05
    done: 2026-11-05
    evidence: build/logs/board--2026-11-05T13-12-18
    note: "panel opens: the layer's paint is the only raster, base_rows=0; the alert alone at 322x130 (paint 42 ms) on build/logs/usb-visible--2026-11-05T12-35-43"
  - id: P6
    story: "a modal and a toast stay in front of every window: the shell's overlay layers are drawn above the windows, the bar and the panels, a modal's scrim is one texel the compositor spreads, and the toast never covers the bar"
    state: done
    gate: "board: apphost motion layer front=1 backdrop=1 for the power alert at its sheet's size (not the screen's), front=1 for the toast; the alert opens beside an open Control Center without a Tier-2 fade; host test_reject_a_backdrop_naming_a_missing_patch, a_motion_layer_with_a_backdrop_changes_the_whole_surface"
    started: 2026-11-05
    done: 2026-11-05
    evidence: build/logs/usb-visible--2026-11-05T12-35-43
    note: "alert front=1 backdrop=1 at 322x130, toast front=1 below the bar, scrim to 56 %; confirmed by the operator on the board 2026-11-05 (trace incomplete past 300 KB)"
  - id: P7
    story: "a window's frosted base can carry a motion layer: a sheet entering in an app window is moved by windowd like the shell's panels"
    state: done
    gate: "board: a window sheet's enter has no apphost hops motion episode; apphost motion layer for it"
    started: 2026-11-13
    done: 2026-11-16
    evidence: build/logs/board--2026-11-16T15-53-32
    note: "Board board--2026-11-13T13-50-14 (image dev-1e3cb775): the log names the refusal — apphost: motion in app (token=10 why=resets) x6 for the window kit's menus and (token=3 why=resets) x7 for a window's fades: a blur root on the window's frost resets its rectangle (motion_math::composes_apart), so no window carries a layer yet (all 20 layers are the shell's). The app's own path got cheaper: its damage is what moves and paints (motion_math::moved_extent) — a menu's enter repaints 136-259 rows at 8-15 ms a frame (median; max 14-32), before 604-698 rows at 37-42 ms. A window's page fade still repaints 464-988 rows at 22-59 ms. Open decision (ARCH, the operator's): a window's popover as a layer WITH a backdrop — windowd blurs under a window's layer and the menu frosts the window's content, a look change against painting in place; the same for a faded page that holds a pane. 2026-11-16: the window kit's popover (a blur root, .material(overlay)) was refused on a window's frost (why=resets, 4-7 per board cycle, board--2026-11-16T15-13-23) — composes_apart now lets a blur root through: the layer claims its glass region and windowd blurs under the moving layer what lies below it in the window (the design's frosted popover); only a faded opaque pane resets. Board pending: no why=resets, the menus' enter without a tokens=menuIn hops episode BOARD build/logs/board--2026-11-16T15-53-32: gate met — no why=resets (were 4), no tokens=menuIn hops episode; the window menu is a layer windowd moves (apphost: motion layer token=10 w=383 h=353 paint_us=11587 once; windowd: owner=4 ms=280 scale=9200 dx=-14 dy=-12); frames 51/22, inputs 102/9, launches 6/0 as before. Closes with the commit (rule 8) Operator 2026-11-16: looked right and felt smoother"
  - id: P8
    story: "closing a panel, the launcher or a toast costs no app raster: the layer goes and nothing under it is painted again - the cause of today's close frames named first (an exit motion, a store replan or a scrim), then cut"
    state: proving
    gate: "board: the launcher's close without an apphost hops motion episode (was 4 frames of 1920x1080 at 96 ms each with tokens= empty, board--2026-11-05T13-12-18); QEMU: the toast's timeout close without one (was 413k px)"
    started: 2026-11-18
    note: "2026-11-18 board cycle 37 (board--2026-11-18T16-23-19): dismissals (2 timeout, 1 Escape) bounded; of 49 layout taps 21 name rows and 28 name the whole base (base=whole), and 13 of 15 re-made surfaces paint the whole base (9 store-only with their rows carried) — the open part: a tap that changes the base at its top (bar button) and far below is ONE row span = every row. QEMU usb-visible--2026-11-18T16-08-51: timeout, backdrop and Escape base=0..0 with rows=0 episodes (were 800 rows, 75 ms); opens base=whole(130/135/42/46), whole(135/141/42/41), whole(141/161/39/0), whole(65/140/2/0). Built: overlay.rs apply_dismiss_damage bounded (relayout_bounded; tick + timeout = rows of both), layout_diff::part_rows (base and layers compared apart; test a_change_at_the_start_and_a_held_overlay_at_the_end_damage_the_start_alone red before), tests split to layout_diff/tests.rs. Next cut: the base's change as several spans (the present carries 4 rects; dirty_rows is one span through the event loop) so two changes far apart stay two; then the store's re-create for a grown layer set (9 per run) without a base paint. Instruments: 'apphost: tap (...) base=', 'apphost: surface re-made (store_only= store_rows= carried= base_paints= layer_rows=)'. 2026-11-18 PACKAGE DESIGN (arch result build/agents/arch-shell-transitions-2026-11-18.md): three causes in order — the diff window (part_rows unions every box between prefix and suffix, layout_diff.rs:330), the 80 % rule (layout_diff.rs:272 → full redraw), then the one span. Cut: band_math::Pending{whole, rects: PresentDamage} (4 rects, cheapest-pair merge) replaces dirty/dirty_rows/tap_render_span/Cols/Changed.base; layout_diff aligns the window (greedy resync, LOOKAHEAD 64, ALIGN_BUDGET 8192), unmatched box = painted rect; no 80 % rule. Re-creates (0.3-0.45 s each on the board): derive outgrows from the last plan, monotone, desktop reserves MAX_SURFACE_ROWS; line surface re-made (why= plan= held= us=). BUILT 2026-11-18 (package, uncommitted): QEMU build/logs/usb-visible--2026-11-18T23-25-19 (39/39 markers): the shell opens name rectangles — base=rects(n=1 px=1056) (was whole(130/135/42/46)), rects(n=2 px=2656) (was whole(135/141/42/41)), rects(n=4 px=33504) 176 rows 7.4-9.7 ms (was whole(141/161/39/0), 800 rows 75-88 ms; above the 64-row target: the rows' per-row floor, TASK-0367 P3); a page change in an app window stays one surface rect (65/140/2/0, legitimately); 0 desktop store re-creates (the 2 surface re-made lines are window resizes, why=resize). Board pending. BOARD build/logs/board--2026-11-19T10-18-04 (dev-9982c5a1, cycle 3): 0 base=whole( on any tap (was 28 of 49), 0 desktop store re-creates (5 store_only re-creates are app windows growing on demand), bar taps 1156 px except one panel open at 33504 px (4 merged rects; gate 20000), closes paint no base row. Operator: 'alle Probleme scheinen gelöst, fühlt sich sehr flüssig an'."
  - id: P9
    story: "a transition leaves the way it came: panels, sheets and their scrim, the island, toasts, the launcher, overlay panes and the action bar animate out on the compositor (today a .transition is enter-only)"
    state: proving
    gate: "host: each exit sampled at named times for every enter token (0/10/50/100 %), a re-opened close keeps place and velocity (|dv| < 2 % of peak), test_reject_a_reserved_motion_flag, test_reject_a_pointer_for_a_leaving_front_layer, test_reject_an_unbounded_turn_velocity; QEMU usb-visible: each dismissal shows windowd: motion layer (... exit=1 with rows=0; board: each close shows exit=1, no apphost hops episode with rows > 0 during it, and frames dropped on a close no more than on the same item's open"
    started: 2026-11-18
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): primitives-window AW-7; shell-bars TS-12; launcher-lock C3 E14; launcher-lock L11 L34 L51; settings-stash A11; settings-stash F22; P5 transitions M05 M23 M26 M30 M36 M57 M58 M60 M61 2026-11-18 package 'a shell transition costs what moves, opening and closing' (with TASK-0354 P8, TASK-0367 P3, TASK-0351 P15; judged with TASK-0355 on one board cycle). Design by the architect agent before code: exits cross nexus-display-proto (MotionLayerDesc) and ADR-0097's 'out of scope: exits'. DESIGN (arch result build/agents/arch-shell-transitions-2026-11-18.md): no DSL grammar change (.transition already names remove/close); wire MOTION_EXIT=0x02 in MotionLayerDesc flags (32 bytes kept, reserved bits refused, the id is the layer's life); windowd per-layer state {start, exit, turn(q0,v0), b0}, exit = 1-E(p), a flip mid-motion continues on P0's closed-form spring, exiting front layers own no pointer; app-host keeps a leaving layer's store place until its deadline on its own timer (no IPC back), revive by identity; store_alloc.rs keeps places stable. Ships: panels, Control Center, launcher small/big, toasts, modal sheets + scrim, window menus. ADR-0097 amended. BUILT 2026-11-19 (package, uncommitted): wire MOTION_EXIT (proto 131 tests); windowd motion_geometry/turn.rs — exit = 1-E(p), a flip mid-motion turns on the closed-form spring, exiting front layers own no pointer, lines motion layer (exit= tap_us=) and motion done (frames= of= late_max_us= damage_px_max=) (359 tests); app-host probe/motion_layers/leave.rs + wire.rs — a layer whose node leaves stays a ghost (same id, store place via store_alloc, kept regions/scrolls), declared exit=1 with from_opacity 0, dropped on its own timer; decisions in src/motion_exit.rs (4 host tests). QEMU build/logs/usb-visible--2026-11-19T08-52-49 (39/39): 6 closes, each apphost: motion exit + windowd exit=1, 30-32 frames a close, no app raster for a close (base=none), no exit cut; tap_us 70-96 ms (QEMU). build/logs/many-apps--2026-11-19T08-57-50: 15/15, layers(refused=0) x56, no re-create. Known: dropping a played-out layer recomposes its rect once (with a scrim: the surface) — GPU only. Board pending. BOARD build/logs/board--2026-11-19T10-18-04: 24 closes (apphost: motion exit), 14 played by windowd (exit=1; the rest cut by a following layer, exit cut why=room x8), no app raster during a close; desktop closes drew 94/263 frames (36 %) vs opens 121/464 (26 %) — no more dropped than the open (gate met); glass and shadow fade with the layer (fixed in cycle 2/3)."
  - id: P10
    story: "the transforms the tokens lack: a rotation (chevrons, disclosure triangles, the floating button's 45 degrees), a translate in percent of the own width (slider and progress fills, the gauge), one part of a widget animating alone with a pop token (the toggle's layer, segment and slider thumbs, check and radio marks, PIN dots)"
    state: planned
    gate: "host: samples per new property at named times, goldens; board-visual"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): primitives-core CHK-8 RAD-6 SEG-4 SLD-3 SLD-10 PIN-3 PRG-4; P5 transitions M39 M40 M41 M42 M43 M12 M13 M14 M51 M52 M62 M63 M64 M69"
  - id: P11
    story: "a menu opens from its anchor's corner (a menu-in token: 92 %, a quick spring), children enter staggered (the island's bars 80 ms, the floating button's minis 40 ms), a press scales by category (95 % buttons, 96 % tiles, 94 % keys held, 92 % icons, 88 % window buttons) and the loops keep the design's timing (progress pip 1.1 s smooth, shimmer 1.4 s ease-in-out)"
    state: done
    gate: "host: samples; the TASK-0353 P5 inventory's nearest rows become exact; board: 0 of 32 frames dropped"
    started: 2026-11-13
    done: 2026-11-13
    evidence: build/logs/board--2026-11-13T12-18-30
    note: "built 2026-11-13, first cut: menuIn (token 10: opacity + scale from 0.92, quick, spring) grows from the corner the menu hangs on — the body's edge nearer its transition node's edge. On a compositor layer no origin crosses the wire (scale about a point = scale about the centre + an offset on the same curve; the corner rests within 1 px, host-tested); an enter app-host paints itself scales about the same corner (anchor_point: the node's anchor and its damage). WinAppMenuLayer, WinAppMenuOverlay and Stash's SelectionMenu use it. Loops on a time grid (loop_math): pip 1.1 s smooth, shimmer 1.4 s ease-in-out (LOOP_SHIMMER), no spring. Board 2026-11-13 (board--2026-11-13T12-18-30): a menu in a window runs in the app's path (its frosted base carries no layer, P7) — seven menuIn episodes, 21-58 ms median paint a frame; the first cycle (T11-54-25) showed the corner missing on that path, built in before the second. Left: staggered children (island bars 80 ms, FAB minis 40 ms), press by declared role (a key 94 %, a tile 96 % — shape cannot tell them apart: needs a role, a DSL change), the pip and band entering from outside their track (needs a rounded clip in the row painter, with P10's translate in percent), the lock field's shake (500 ms)"
  - id: P12
    story: "state-bound transforms: sliding indicators, the rail, the page control's capsule, sheet detents with a rubber band, a list row's swipe, the paged track and the launcher's page turn, the calendar's month slide, the island's morph"
    state: planned
    gate: "host tests per transform; board: 0 of 32 frames dropped per transition"
    note: "moved from TASK-0353 (2026-11-10, the operator's \"move them, then close\"; ids of the design spec and the P5 transition inventory): primitives-window TB-8 AB-7 SHT-1; shell-bars IS-07; shell-panels PN-MONTH; launcher-lock G6; launcher-lock F7; P5 transitions M10 M22 M31 M32 M53 M54 M55 M74 M75 M76 Operator 2026-11-19 (board--2026-11-19T09-44-05): the fullscreen launcher's pages, when dragged, follow to the edge and — with no second page — come back on a rubber band (today the paged track stops hard)."
  - id: P13
    story: "what the first cut of P11 left: children enter staggered (the island's bars 80 ms, the floating button's minis 40 ms), a press scales by a declared role (a key 94 %, a tile 96 %), the progress pip and the shimmer band enter from outside their track under a rounded clip, and the lock field shakes for 500 ms on the design's curve"
    state: planned
    gate: "host: samples per stagger and per role at named times; the row painter's rounded clip byte-equal to the SDF at the capsule's ends; board: 0 of 32 frames dropped"
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
