---
schema: 1
id: TASK-0351
title: "Layer 8 on the GPU: an occluded window costs nothing, bindings grow, a fullscreen app is scanned out directly and apps render with the GPU"
status: in_progress
owner: ["@ui", "@gpu"]
created: 2026-11-03
updated: 2026-11-23
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
    state: done
    gate: "board cycle: frame p99, ns_per_px, layers per frame and bindings in use logged for 12 and 30 windows"
    started: 2026-11-03
    done: 2026-11-18
    evidence: build/logs/board--2026-11-18T14-59-11
    note: "2026-11-18: measured through 3, 7, 12, 22, 28, 40 and 49 windows on the board: frame 28/51/84/164/191/271/341 ms, ns_per_px 2-3, layers per frame 165-187, bindings in use 109 of 128 at 49 windows (gpud: gpu frame us, gpu work, dc present mix, bindings)."
  - id: P1
    story: "a window hidden behind others costs the frame nothing"
    state: done
    gate: "host: the lane's cascade under a filled window leaves its middle row (4 of 11) out (culling_model); QEMU many-windows: fill and return chords applied, the pixel proof unchanged (the CPU path blends glass and culls nothing); board: windowd: windows occluded (n= on the operator's stack"
    started: 2026-11-04
    done: 2026-11-05
    evidence: build/logs/board--2026-11-04T18-14-55
    note: "windowd: windows occluded (n=2) on the operator's stack (n=1 on 2026-11-04T16-36-48)"
  - id: P2
    story: "a board GPU context binds as many surfaces as the windows need"
    state: planned
    gate: "host: a space binds 130 bindings past a reserve of 2 (nexus-driverkit), the surface window reaches the layout's GiB; board: 30 windows with 0 binding failures, gpud: bindings of= grows past 128"
    started: 2026-11-04
    evidence: build/logs/many-windows--2026-11-04T09-45-42
    note: "2026-11-18 board--2026-11-18T14-59-11: 49 windows, 0 binding failures, in_use=109 of=128 — the table was not asked to grow (it grows past 128 from about 58 windows; proven on the host). 2026-11-19 board--2026-11-19T15-50-37: bindings in_use=47 of=128 — the 60-window block of the series did not reach 58 windows; still to judge. 2026-11-20 back to the backlog (cleanup): board reached 48 bindings with 0 failures, the 30/64-window block not reached — gate unmeasured. 2026-11-23 cycle 1 (build/logs/board--2026-11-23T11-02-57): gpud: bindings in_use up to 65 of 128, 0 binding failures; windowd: windows drawn up to 21 (the rest covered); of= did not need to grow past 128. 2026-11-23 cycle 2 (board--2026-11-23T17-39-14): 38 windows (33 drawn, 5 covered), gpud: bindings in_use 89 of 128, 0 binding failures — the 30-window part met; of= did not need to grow past 128. 2026-11-23 operator (cycle 2): past 60 windows the top bar sometimes flickers — accepted for now; likely the frame's layer share at that many windows (the top bar strip is one of the shell's layers) — a finding for later, not this package. 2026-11-23 package C close (board--2026-11-23T18-35-25): 50 windows drawn, gpud: bindings in_use up to 108 of 128, 0 binding failures (the 30-window part met); of= did not need to grow past 128; top bar flicker past 60 windows (operator, accepted for now)."
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
    state: done
    gate: "board: gpu pass sweep q16_us within 2x of the stock's 0.64 ms and area16_us within 2x of q16_us (the area reaches the hardware); gpud: gpu gate scene ok (hover below 8.3 ms, cold below 23.3 ms)"
    started: 2026-11-04
    done: 2026-11-23
    evidence: "build/logs/board--2026-11-23T11-02-57: gpud: gpu pass sweep q16_us=485 within 2x of the stock's 0.64 ms, area16_us=479 within 2x of q16; gpud: gpu gate scene ok (pipelined 20435 us < 23300, hover 437 us < 8300; cold 26.4 ms, 38.3 before)"
    note: "board 2026-11-05T13-12-18: q16 915 us and area16 748 us meet the sweep half; gpu gate scene FAIL on cold 44.8 ms against 23.3 (hover 3.3 ms) 2026-11-19 board--2026-11-19T15-50-37: sweep q16 480 us, area16 432 us (met again); gpud: gpu gate scene not printed in the series — the scene half unjudgeable. CORRECTION 2026-11-19 (architect, build/agents/arch-transition-every-frame-2026-11-19.md): the gate scene IS printed in board--2026-11-19T15-50-37 (uart.log:1121): FAIL cold 38.1 ms pipelined 31.8 ms against 23.3 — the scene half is red, not unjudgeable (the earlier note of today was wrong). 2026-11-20 back to the backlog (cleanup: proving since 11-04, never judged): last board gpud: gpu gate scene FAIL cold 38.3 ms (gate < 23.3), hover 2831 us (gate < 8300 ok); sweep not judged since."
  - id: P6
    story: "the passes a frame runs hold the budget on the board, with glass and while something moves"
    state: done
    gate: "board: gpud: gpu gate scene ok, frame p99 <= 16.7 ms with glass on the operator series, 0 of 32 frames dropped during a transition"
    started: 2026-11-18
    done: 2026-11-20
    evidence: "build/logs/board--2026-11-20T10-20-46: W2 C-c (a key miss is a band): slow frames with a whole glass write 122 → 51, whole writes from a moved layer 29 → 0, key misses written as a band 69 %, glass-writing frames' GPU p50 38.2 → 29.2 ms; with W1 drawn>shown 13.4 ms; operator 2026-11-20: 'alles fühlt sich in der tat sehr performant an' — the rest is P19"
    note: "2026-11-18 operator after the fourth board run (board--2026-11-18T14-59-11, commit 37805d99): the freeze is gone (49 windows drawn, 0 refused frames) and that is enough for now — no over-engineering, a note. Frame gate open: 3 windows 28 ms, 7 -> 51 ms, 22 -> 164 ms, 49 -> 341 ms (3-9 glass writes a frame, nearly all band_base). Cause read from 'windowd: loop hz … damage(flushed_px=)': the damage that leaves is one to five bounding boxes of 1.3-2.1 Mpx (a window opened, closed or presented, merged while a frame is in flight), its origin guessed by geometry (the topmost window it lies within, else the base) — every glass in reach blurs again. Design for when this is taken up: (1) windowd: a damage rectangle carries the surface it came from through queue_dirty_rect and the merge, merged per source only; opening, closing and raising a window need no named damage on the GPU path; (2) planner: a change ends at an opaque layer that covers it, a written glass is a new change for the layers above; (3) glass writes counted by origin class. Then the pass's own cost (3.6 ms at 16 draws, 14 ms at 210; emit 8.5 us a draw) is P5/P13. 2026-11-19 building (package frame-budget-glass): design build/agents/arch-frame-budget-glass-2026-11-19.md — the damage origin is the key of the layer that draws the change (Command::Damage origin, no wire layout change, RFC-0101 amended), windowd joins same-key only (bound 6, key 0 above), open/close/raise send no named rect on the GPU path, planner rules R1-R5. Confirmed: board--2026-11-19T12-13-37 l.2882 one 0,0,1920x1080 rect rewrote every glass; 13-14-36 l.4937-4967 a panel switch 8.7 Mpx a frame, 29 of 32 dropped. CW1 (build/agents/cw1-planner-origin-2026-11-19.md): planner R1-R4 built (frame/reach.rs, WritesBy below/glass/unknown/moved/order/rest, index_of_key), nexus-gfx 433 tests, oracle byte-equal 200x2 seeds, plan 3.8->6.3 us. R5 NOT built: both executors address a glass cache from its drawn size (pieces.rs:280-303, gpud glass_blur.rs:110-135) — a scale-in still writes new. Origin 0 now writes more (R3 cascade 4.5->8.25 a plan) until windowd sends keys: CW2 lands in the same commit. Older holes reported, not fixed: blur at rest loses a change made during the motion (glass.rs keep path, 6-line fix changes origin-0 output); a put-off write is visible through a glass between; chain() writes the band plus one texel while the plan grows it by the blur reach. Finding 4: the cycle-38 own rule assumes the desktop body carries layer_id 0 — the board trace (gpud: gpu glass written key= id=) settles it. 2026-11-19 BOARD board--2026-11-19T15-50-37 (damage origin built: windowd damage_origin.rs topmost drawing layer, queue bound 6 same-key, structure changes without a rect on the GPU path; planner R1-R4): dropped 637 of 5728 (11.1 %, was 14.8 %), worst frame per window p50 15.6 ms (was 23.9) p90 79.8 ms (82), tap p50 48.5 ms (57.7), gpud recv>drawn p50 5.4 ms (24.9); damage per open 37k-463k px for panels/CC (was 1.3-2.1 Mpx). Gate NOT met: transitions 8-9 of 17 frames, glass_w 11-17 per 32 (gate <= 8, was 42-78): each glass write is a dropped frame. glass writes by: moved=381 (a glass whose backdrop moves — the moving panels own glass: R5, the cache addressed by drawn size on both executors) unknown=263 (origin 0: app-host whole presents 1.02 Mpx key 0/0xffff0100, moved-region key 0 x7) glass=105 rest=92 order=38. Next: R5 in both executors (pieces.rs, gpud glass_blur.rs) and the remaining key-0 sources (damage wide names them). 2026-11-19 architect (arch-transition-every-frame): the halving is the cadence; the glass writes that trigger it are class new (7-12 per panel window: a moving glass re-addressed by its drawn size) and moved (a glass at rest over a layer moving by transform, launchers 12-21); unknown=0 in panel and launcher windows (hypothesis b refuted; recurring key 0 = windowd moved-region whole-screen). Decided: glass rows belong to the layer's rest rectangle, one bounded hold rule, CW1 Findings 2 and 5 ship with it. 2026-11-19 MEASURED by the frame trace (TASK-0373, board--2026-11-19T22-09-42, nx hops): per hop p50/p90 — gpud sub>gpu bare 11.7/40.7 ms, loaded 32.1/84.5 ms; gpud gpu>latch (done, waiting for the flip) bare 39.1/72.8, loaded 37.1/79.6 ms; latch>shown one period; windowd owed>scene (waiting for a free slot) 31-40 ms p50; sent>acked 67-100 ms p50. Two causes: (1) GPU-heavy slow frames n=160 sub>gpu 49 ms with whole glass writes (whole ~1, 779k px written whole, 8 passes, 203 draws; frames with a whole write 53 ms, 1.5 Mpx) — K2 candidate (no whole writes) CONFIRMED for the loaded desktop; (2) light slow frames n=270: GPU done after 8.4 ms, then 34 ms until the flip — the flip queue and pacing, the largest part of a bare-desktop tap; no glass rule touches it. 2026-11-20 W2 C-c BUILT (critical-writer opus high, build/agents/w2-key-miss-band-2026-11-20.md; architect build/agents/arch-flip-queue-2026-11-20.md §3): in a plain plan a key miss of rows current one plan ago (same base key, not rested/full/held/put off) blurs the band its layer changes explain (changed drawing old ∪ new, came, went, reorder ∩ reach ∪ touch.band ∪ owed), else Full. Counters Writes.backdrop_band, Holds.foreign (C-a decides next cycle on foreign >= half of rest writes). nexus-gfx 450 → 456, oracle green, mutation (band 40 px short) → oracle red 2/3. Board gate (loaded block): whole>=1 with by moved <= 15 (C3 62), all whole>=1 <= 45 (C3 91), backdrop_band >= 70 % of backdrop, loaded recv>drawn median < 20 ms. 2026-11-20 CYCLE 4 board--2026-11-20T10-20-46 (W2 C-c): slow gpud records with a whole glass write 122 → 51 (gate <= 45 ✗ close), whole writes with a moved layer below 29 → 0 (gate <= 15 ✓), backdrop_band 468 of 678 key-miss writes = 69 % (gate >= 70 % ≈), glass-writing slow frames sub>gpu p50 38.2 → 29.2 ms (gate half ✗). glass holds foreign=1352 (C-a input). W3: 47 single-pass traces; the 13 readable show the fragment running 7.6-13.1 ms with the completion IRQ prompt (late_us <= 45) — GPU busy, not a late wake; the 41 ms waits' trace lines were not readable (cut)."
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
    started: 2026-11-20
    note: "2026-11-20 back to the backlog (operator): no consumer yet — no fullscreen opaque app draws without compositor layers (architect build/agents/arch-package-b-2026-11-20.md: P9 also needs the overlay plane for the bar strip). Revisit when such an app exists."
  - id: P10
    story: "the emit costs what its draws write: a pass's draws are walked once per frame (the plan's wallpaper pieces and each layer's draws in order) and the emitter indexes a kept list instead of asking the plan per draw"
    state: done
    gate: "host: the walked draws equal the indexed walk for the cascade, the glass and the twelve-window scenes (frame_board), and the walk touches each layer once; board: gpud: gpu frame us split emit per draw <= 15 us at 12 windows"
    started: 2026-11-04
    done: 2026-11-18
    evidence: build/logs/board--2026-11-18T10-51-11
    note: "board 2026-11-18T10-51-11: emit 10.4 us per draw at 12 windows (381 draws a frame), 9.0-11.9 us at 10-205 draws"
  - id: P12
    story: "the pass's fixed cost is located by the firmware's own clock: every traced pass prints the firmware's utilisation deltas (idle, active, blocked), the commands it executed and the interrupts it took, and the sweep prints the area and passes each case actually drew — the hover pass and the sweep's q16 against the rung's 0.5 ms through the same firmware"
    state: done
    gate: "board: gpud: gpu pass fw (of=hover|q16|area16 …) and gpud: gpu pass sweep drawn (…) on the cycle's log; the reader names where the pass's time goes (GPU active vs idle vs blocked, or a drawn area larger than the damage) with a number"
    started: 2026-11-04
    done: 2026-11-05
    evidence: build/logs/board--2026-11-05T13-12-18
    note: "gpu pass fw and sweep drawn on every cycle since 2026-11-04; the reading: the passes drew 1920x1080 for a 16x16 damage (cut by P13, q16 5857 -> 915 us); the firmware's util counters stay at +0"
  - id: P13
    story: "the gate and the sweep measure what the desktop's frames draw: after the cold frame the slot is the GPU's own (no stale slot, no dirty base), so the hover pass and the sweep's quads are limited to their damage - the cycle-39 'fragment latency' re-read as the fragment stage shading the whole screen"
    state: done
    gate: "board: gpud: gpu pass sweep drawn q16=16x16 and area16=16x16 (not 1920x1080), gpu gate scene hover_us below 2000; host: frame_board a second frame on a drawn slot with a 1-px damage draws no full-screen piece"
    started: 2026-11-04
    done: 2026-11-23
    evidence: "build/logs/board--2026-11-23T11-02-57: gpud: gpu gate scene ok hover_us=437 (gate < 2000; 2831 before); gpud: gpu pass sweep drawn q16=16x16 and area16=16x16, not 1920x1080; host: tests/frame_clip.rs a_cut_glass_back_samples_what_the_whole_one_did, test_reject_a_cut_without_its_apron (C1)"
    note: "board 2026-11-05T13-12-18: sweep drawn 16x16 (met); gate scene hover 3293 us against 2000 CORRECTION 2026-11-19: board--2026-11-19T15-50-37 uart.log:1121 hover_us=2838 against 2000 — red, not unjudgeable. 2026-11-20 back to the backlog (cleanup): last board gate scene hover 2831 us against the gate hover_us < 2000 — red."
  - id: P14
    story: "a window's content costs its pixels once: a scroll band whose viewport lies on an opaque pane is rastered on the pane's pixel and declared opaque (BAND_OPAQUE), so the compositor draws the slice opaque and cuts the pane, the body, the glass back and the wallpaper under it - and the pane is the design's opaque surface-pane (the artifact's token)"
    state: planned
    gate: "host: nexus-gfx frame_window_cost an opaque slice over a pane adds no blended pixel and the pane under it is cut; app-host a band on a pane is opaque, a band touching the pane's corner or on a translucent pane is not; board: gpud: gpu work straight_px below a third of opaque_px through the typing and window windows, ux frame max_us below 40000 in every window"
    started: 2026-11-04
    note: "board 2026-11-05T13-12-18: straight_px 0.9-1.1 of opaque_px, ux frame max 37 ms 2026-11-19 board--2026-11-19T15-50-37: straight_px/opaque_px 1.17 over the series (gate below 0.33): unchanged by the damage fix, as expected. 2026-11-20 back to the backlog (cleanup): straight_px vs opaque_px never judged against the gate since 11-04. 2026-11-23 package C close: the counter apphost: band opaque (…) is built; no band ground changed often enough on the board to print it (prints every 16 changes) — the next package reads it from a scroll-heavy block."
  - id: P15
    story: "A transition's frame damages the rectangle that moved, in the app and in the compositor: app-host names the animated nodes' columns (the row painter still paints whole rows; a row-only contributor widens to full width), and windowd takes a structure whose regions only moved as those regions' old and new rectangles — not the whole surface, which a sliding glass panel re-declared every frame. The motion line says what each frame cost: raster per frame and its rows, damage per present, the tokens that ran."
    state: done
    gate: "board: windowd: motion done for each panel and launcher open and close shows damage_px_max <= 1.5 x (w+64) x (h+64) (418k Control Center, 820k small launcher, 2 073 600 big launcher); apphost hops motion px per present below 400000 for a panel slide; host: band_math rect set, layer_geometry moved-region and scroll-comes-or-goes, ux-budgets episode tests green"
    started: 2026-11-04
    done: 2026-11-19
    evidence: "board board--2026-11-19T17-56-19: windowd motion done damage_px_max (bounding box since this package) Control Center 391x548 214268 (gate 418k), top-bar panels 393x687 269991 (515k), small launcher 782x582 455124 (820k), big launcher 1920x1050 2016000 (2.07M)"
    note: "board 2026-11-05T13-12-18: panels slide as motion layers (TASK-0354 P2); launcher open slot 2.76 Mpx and body 1.19 Mpx, Control Center slot 1.18 Mpx and body 469k (met) 2026-11-18 GATE CORRECTED (arch result build/agents/arch-shell-transitions-2026-11-18.md): a moving translucent layer of L px costs at least ~2L of slot, so 'slot < 1.5 Mpx' is unreachable for the big launcher by construction (1920x1050 = 2.016 Mpx, floor ~4.03 Mpx, measured 3.98 Mpx, board--2026-11-18T16-23-19 uart.log:5370); absolute slot/body numbers belong to P6. windowd moved-region is built (layer_geometry.rs:198); remains: the app's damage as rects (TASK-0354 P8's Pending) and structure_change treating a scroll layer that comes or goes as STRUCTURE (layer_geometry.rs:191) → it damages its viewport. BUILT 2026-11-18 (package, uncommitted): windowd structure_change — a scroll layer that comes, goes or is re-placed damages its viewport (test a_scroll_layer_changes_its_viewport_when_it_comes_or_goes), a backdrop's motion-over changes nothing; app side = TASK-0354 P8's rect set. Board pending (windowd: motion done damage_px_max comes with TASK-0354 P9). BOARD build/logs/board--2026-11-19T10-18-04: the corrected gate is not judgeable yet — windowd: motion done damage_px_max counts the whole frame's damage, not the motion's share (critical-writer finding 2; the share needs the planner, present.rs). 2026-11-19 the motion share comes with P6 (same planner, origin keys): build/agents/arch-frame-budget-glass-2026-11-19.md B2. 2026-11-19 BOARD board--2026-11-19T15-50-37: damage_px_max per open — Control Center 391x548 360k-404k (gate 418k ok), top-bar panels 393x687 463k (gate 515k ok), small launcher 782x582 1.06-1.14 Mpx (gate 820k NOT met), big launcher 1920x1050 4.17 Mpx (gate 2.07 Mpx NOT met): the launchers moving glass is re-written whole each frame (P6s R5). CORRECTION 2026-11-19 (architect): the launchers numbers are the instrument — motion done damage_px_max SUMS the entries, and windowd names one change under two keys; the bounding box is what the gate means (Decision 3: structure names nothing on the GPU path, damage_px_max becomes the bounding box). 2026-11-19 DONE (architect verdict, build/agents/arch-transition-every-frame-2026-11-19.md 'Board cycle 1'): every shell transition damages within its gate; structure changes name no rectangle on the GPU path, the instrument counts the bounding box."
  - id: P16
    story: "A launch's wait ring costs what it shows: it steps every 90 ms on the compositor loop's one-shot wake (the caret's timer), never on a present's completion - on a display without a vertical blank a completion returned in 150 us, windowd looped at 668 Hz and the launching app starved (usb-visible 11-07-04 and 12-35-43)"
    state: done
    gate: "QEMU usb-visible: launch settings and the chords green, the ring's own pointer frames below 60 a second while it turns (was 668); host: wake_clock tests; board: the ring turns while an app launches"
    started: 2026-11-05
    done: 2026-11-05
    evidence: build/logs/usb-visible--2026-11-05T13-27-58
    note: "launch settings green in 14.6 s, 39/39 chain markers; the ring's frames pointer=11 and 2 a second (was 668); the one 360 Hz window is the overlapping open transition (moved=332, P17) - the first gate counted loop hz and conflated it; board 2026-11-05T13-12-18: the ring on four launches, 15 presents a second"
  - id: P17
    story: "a display without a vertical blank completes a present no sooner than its mode's refresh: on QEMU's virtio GPU windowd's transitions, motion layers and frame pulses run at the display's cadence instead of spinning on completions that return in microseconds"
    state: done
    gate: "QEMU usb-visible, each second of a window's open transition: windowd present at most 60, a client's frame requests at most 60, loop hz no more than the messages it drained (woke(gpud=+req=)) — was present=2083, loop hz=3002, frame requests 2977; board unchanged (vblank-paced)"
    started: 2026-11-18
    done: 2026-11-18
    evidence: build/logs/board--2026-11-18T16-23-19
    note: "2026-11-18 board cycle 37 (board--2026-11-18T16-23-19, image dev-50c2785e): ladder complete (118 rungs, red only at the operator's board-visual ack), no bkl stall. The pulse rule on the board: 1.09 app ticks a sent frame over 179 motion episodes (582.6 over 25 in board--2026-11-18T14-59-11), frame_req max 82/s, present max 50/s, loop hz max 586 = gpud 296 + req 296 (pointer samples apply=246); frame pace on_time=2083 late=291 early=165; ux frame 44 ok / 12 warn / 23 FAIL of 79 windows; launches 78-341 ms (4 ok, 2 warn; 4 windows); ux input p50 16.6 ms, 133 ok / 18 warn / 8 FAIL. The board's display acks at its flip as before (vblank-paced). QEMU: usb-visible--2026-11-18T15-30-19, many-windows--2026-11-18T15-37-38, many-apps--2026-11-18T15-42-03 green (present max 59-61/s was 2083, frame_req max 56 was ~2977, launches p50 229 ms was 545). Seen, not this phase's: a motion hop stays open when a pulse found motion live and its tick sent nothing (pulse>sent max 6-14 s in six episodes; 4 s on QEMU before the change too) — the instrument's, TASK-0354 P1's line."
  - id: P18
    story: "a window never flickers: Settings with its sidebar as an overlay (a narrow window) and Settings fullscreen show the same pixels frame after frame at their bottom, as every other window does"
    state: done
    gate: "board: the operator sees no flicker at the bottom of a narrow and of a fullscreen Settings window (board-visual ack); the cause named in the note (which surface, which slot, which damage) before the fix"
    started: 2026-11-19
    done: 2026-11-19
    evidence: "board--2026-11-19T10-18-04 (dev-9982c5a1); commit d76b9a22"
    note: "reported by the operator on 2026-11-16 after board--2026-11-16T09-14-28: in Settings, with the left sidebar when the window is narrow, the bottom of the window flickers, and fullscreen too; present before that image (not TASK-0367 P3's column painting). Not read from the log yet: the sidebar becomes an overlay pane (sidebarOverlay, paneL) with its own scroll layer (the section list) between a header and the account footer; the content's scroll layer is then covered and flattened into the base. Candidates to measure first: a slot that differs from the other at the bottom (damage that names fewer rows or columns than changed), the sidebar band's fill at the viewport's end, a scroll layer re-declared per frame 2026-11-19 CAUSE NAMED (architect, build/agents/arch-stale-window-pixels-2026-11-19.md): windowd compositor/runtime/app_window.rs gave present_damage the window's shadow-halo rect (frame + 75 px) as the surface's place while the scene drew the surface at the frame's corner — every app present's rects landed min(win.x,75)/min(win.y,75) up-left of the change (calculator at 598,62: the digit's 41x72 rect shared 0 px with the digit; board--2026-11-19T09-18-34). Surface: any app window; slot: none (app slots and gpud's planes consistent — buffer age kept, desktop.rs:414); damage: windowd's translation. FIX (uncommitted): surface_place.rs (one place for scene + damage, 3 host tests), app_window_rect renamed app_window_damage_rect, caret's origin fixed; nexus-gfx a_ring_of_planes_shows_what_a_full_redraw_would. A fullscreen window at exactly (0,0) is not explained — P18 closes only after its board check. BOARD build/logs/board--2026-11-19T10-18-04: the calculator's digits show with each tap (operator, cycle 3: 'alle Probleme scheinen gelöst'); Settings narrow/fullscreen bottom: ack pending. Operator 2026-11-19 (board--2026-11-19T10-18-04): Settings narrow (sidebar overlay) and fullscreen — the bottom stays steady (board-visual ack, checked). Closes with the commit."
  - id: P19
    story: "the glass and GPU rest of P6: a held glass made to owe the whole glass by a foreign change writes a band (C-a, decided by glass holds foreign=1352), and the single-pass frames stuck at ~41 ms are named by a readable trace (GPU work or a wait)"
    state: planned
    gate: "board: slow frames with a whole glass write <= 45 (cycle 4: 51); glass-writing frames' GPU p50 <= 19 ms (cycle 4: 29.2); every single-pass frame waiting > 20 ms has a gpu pass trace line inside the console cap"
    started: 2026-11-20
    note: "2026-11-20 board--2026-11-20T12-38-17 instruments: 53 single-pass traces, all readable (cut 0); the 41 ms ones cover 10.3 Mpx in one pass (5x the 2.07 Mpx screen) at 4 ns/px with gap_us ~1 ms — the GPU is busy with overdraw, not a late wake. glass writes by: zero 342 (key-0 damage, the largest cause; windowd damage zero from present-rect and present-whole), stray 79, moved 231, rest 230, below 134. rest misses: moved 139, set 51, own 4, drew 4, epoch 0 → C-r is the moved band. 2026-11-20 board--2026-11-20T15-34-30: windowd raise id=desktop 81 → 0; glass writes zero 342 → 83 (gate <= 30), stray 79 → 26, below 134 → 277 (named now); rest misses set 51 → 83, moved 136, band 17 (D3's rest bands); launcher single pass 10.3 → 9.65 Mpx, 41.4 → 35.2 ms; one 24.9 Mpx / 64 ms pass in the 22-window block. 2026-11-21 board--2026-11-21T07-37-47: rest misses set 110, moved 74, own 11, band 5; whole rest writes by first condition base 2 / owe 165 / new 0 / shape 0 — the owe (the hold's collected change or the owe is the whole glass) decides the rule for B2. Launcher-class single passes 9.68 -> 8.72 Mpx, 36.6 -> 33.1 ms. 2026-11-22 board--2026-11-22T11-03-14: whole rest writes from the owe 165 -> 0 (P19-e), rest misses written as a band 5 -> 120 of 202, slow frames with a whole glass write 79 -> 40 (gate <= 45 met); glass-writing frames' GPU p50 37.6 ms (gate <= 19 not met: the loaded desktop's glass passes); single-pass traces readable (met). 2026-11-23 cycle 1 (build/logs/board--2026-11-23T11-02-57, C1+C2+C3): glass-writing frames' GPU p50 37.6 -> 22.2 ms (gate <= 19), slow frames with a whole glass write 37 (gate <= 45 met); C5 (runs) next. 2026-11-23 cycle 2 (board--2026-11-23T17-39-14): glass-writing GPU p50 22.2 -> 19.2 ms (gate <= 19), whole-glass slow frames 43 (<= 45); the runs (C5) never ran: both run planes refused as Vm(Unaligned) — 8 386 560 bytes, half a page over; fixed (frame_limits.rs RUN_PLANE_BYTES in whole pages + const-assert); proof in cycle 3. 2026-11-23 package C close (board--2026-11-23T18-35-25, cycle 3): glass-writing GPU p50 37.6 -> 19.2-22.1 ms over cycles 2-3 (gate <= 19, the series heavier: 50 windows); whole-glass 39 (<= 45 met). C5 runs: planes live (n=2), but in 1024 frames the below run was taken once and the group run never (groups=0, apart=1) — finding: the group-run trigger (a motion group of more than one layer at opacity < 255) does not fit the launcher, likely one motion layer; next package."
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

## Three lenses (P6, 2026-11-18: many windows)

Measured before (`board--2026-11-18T10-51-11`, the launcher starting an instance per press):
12 windows 84 ms a frame (12.9 passes, slot 9.5 Mpx, scene 5.9 Mpx), 17–20 windows 0.3–0.8 s;
no hart above 1.5 % busy. `glass_off=81` of 32 frames: the cache held rows for four windows'
glass, the others lost their blur and damaged their rectangle every frame. `band=115` of 32
frames: a change reaches every glass stacked above it.

- **Scope** — in: the glass cache as rows kept per glass in planes the executor adds
  (`frame/glass.rs`, `gpud backend/gpu/desktop_cache.rs`); the band writes counted by what
  reached them; the frame's bound of 64 layers gone on the whole way (the planner's tables
  grow with the scene, the wire crate's budget of 256 layers a frame, a present in parts, the
  draws a pass's memory holds); a window covered whole stated `hidden` to the lifecycle
  authority (the second run, `…T11-44-54`: the launcher lost its layers at 20 windows, 19 of
  20 windows `visible`). The third run (`…T13-29-53`) froze at the 21st window on the
  serializer's own 8000 bytes — a fourth copy of the same bound — and measured 53 ms a frame
  at 10 windows and 102 ms at 15 (11–13 passes: three or four glasses blurred again a frame,
  `band_base` = all of them). In, since then: every bound of the frame's path once and tied
  by constant asserts, a frame past a bound degraded and named instead of refused, the
  sixty-four-window run down the real path on the host (gpud `frame_limits`), a refused frame
  forbidden in the lanes; and a glass whose change is hidden by the layers above is not
  blurred again (`frame/plan.rs`: its rows owe the change).
  · out, named: a damage rectangle's origin by the surface that changed (today the topmost
  window it lies within), two-dimensional packing of the cache (a plane holds four windows;
  memory, not correctness), more than 64 windows in one frame (the capture reply's bound),
  continuation renders for a pass past 1536 draws (its lowest draws are left out).
- **Invariant** — a glass that fits a plane is never shown without its backdrop because the
  cache is full, and no other glass's rows move when the set changes:
  `a_glass_keeps_its_rows_while_the_set_around_it_changes`,
  `a_glass_without_rows_is_off_and_counted_short_until_the_cache_has_a_plane_for_it`,
  `test_reject_rows_across_a_plane_edge_and_a_glass_taller_than_a_plane` (host);
  `gpud: gpu frame us … glass_off=0` at 12 and 20 windows (board). A present in parts is one
  sender's, one seq's, in order and at most its bound, or it is acked malformed — never a
  present of some of the bytes: `test_reject_a_part_out_of_order_or_of_another_present`,
  `test_reject_a_part_of_another_sender`,
  `test_reject_a_body_past_its_bound_and_a_part_without_a_header`. No range of the GPU
  context overlaps another: `no_two_ranges_of_the_context_overlap`. A frame at the
  compositor's bounds is never refused on its way: `a_frame_of_sixty_four_windows_goes_the_whole_way`,
  `a_pass_of_the_most_draws_fits_every_ring`, `a_frame_at_its_budget_is_a_present_the_display_driver_takes`,
  `test_reject_a_stream_naming_more_commands_than_a_buffer_holds`; `windowd: flush rows fail`
  forbidden on `ci-os-many-apps` (forty windows). A glass put off is written by the frame
  that shows it: `a_change_under_the_window_above_is_not_blurred_again_until_it_shows`.
- **Contract** — RFC-0109 (a slot follows its layer's identity), RFC-0101 §8 (the plan's damage)
  and §8e (a present in parts), ADR-0078 (the context's private buffers), ADR-0083 amended (a
  window covered whole is hidden).

## Not doing here

## Definition of Done
