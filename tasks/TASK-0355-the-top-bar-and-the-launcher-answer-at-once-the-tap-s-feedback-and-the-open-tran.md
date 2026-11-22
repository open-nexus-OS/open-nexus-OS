---
schema: 1
id: TASK-0355
title: "The top bar and the launcher answer at once — the tap's feedback and the open transition measured, then cut to the budget"
status: in_progress
owner: ["@ui"]
created: 2026-11-03
updated: 2026-11-22
subsystem: ui
components: [desktop-shell, app-host, windowd]
kind: ui
novelty: variant
estimate: 3
started: 2026-11-03
dates_source: recorded
goal:
  metric: "tap to the first changed frame; frames dropped while a top-bar panel or the launcher opens and while the launcher pages"
  baseline: "board 2026-11-03 (board--2026-11-03T19-13-30): tap to submit 77 ms median (15 ok, 20 warn, 20 FAIL of 55 windows); 24 of 32 frames dropped (median); dominant hop gpud sent>acked 90 ms mean, staging 0.06 ms"
  target: "tap to the first changed frame <= 50 ms; 0 of 32 frames dropped while opening a panel, the launcher (small and big) and paging; ux frame p99 <= 16.7 ms"
  instrument: "hops lines (RFC-0108 §hops) · gpud: ux frame · windowd: ux input · board-visual ack"
consumer: TASK-0353
links:
  - label: "Top bar"
    target: "TASK-0312 (topbar + six drop-downs)"
  - label: Launcher
    target: "TASK-0313 (launcher, pager glide, docks)"
  - label: Budget
    target: RFC-0108
phases:
  - id: P0
    story: "the time from a tap on the top bar or the launcher to its frame is measured on the board, hop by hop"
    state: done
    gate: "board cycle (shared with TASK-0351 P0 and TASK-0354 P1): tap-to-frame and frame p99 logged for the 6 panels, the launcher small and big and the pager; the dominant hop named in ms"
    started: 2026-11-03
    done: 2026-11-03
    evidence: build/logs/board--2026-11-03T19-13-30
  - id: P1
    story: "the dominant cost P0 names is cut"
    state: done
    gate: "board: the named hop at most half of its P0 value; tap to frame <= 100 ms"
    started: 2026-11-19
    done: 2026-11-19
    evidence: "board board--2026-11-19T15-50-37: gpud hops frame recv>drawn p50 5378 us over 179 windows (P0 24914 us, board--2026-11-19T12-13-37: below half); tap to the transition's first frame p50 48.5 ms over 70 opens (<= 100 ms)"
    note: "re-measured on the board 2026-11-05T13-12-18 after TASK-0354 P2: a panel's open costs one layer raster in app-host (Control Center 486 ms, launcher 599 ms, a top-bar panel 283 ms), now the dominant hop ahead of gpud's frame (TASK-0351 P5/P6); the operator put TASK-0353 first BOARD build/logs/board--2026-11-19T10-18-04 (2026-11-19, windowd: motion layer tap_us — new instrument): tap to the transition's first frame p50 61 ms over 27 opens, 9 at or below 50 ms, max 493 ms; the big launcher 104-116 ms (its layer raster 35 ms + GPU passes, TASK-0351 P6). Before the package the shell's opens painted the whole base (800 rows) and re-created the desktop's surface (0.3-0.45 s). 2026-11-19 DONE (board--2026-11-19T15-50-37): the dominant hop (gpud's frame) cut by the damage origin package (TASK-0351 P6/P15, TASK-0343 P7 measurement)."
  - id: P2
    story: "the top bar and the launcher open and page without a hitch"
    state: planned
    gate: "board: tap to frame <= 50 ms; 0 of 32 frames dropped on every open and on paging"
    started: 2026-11-19
    evidence: "board board--2026-11-19T12-13-37: paging 18 drags, every glide 500 ms rastered=0 (the paging half holds); tap to open p50 58 ms max 502 ms (n=36; regression guard p50 <= 61 ms holds, the 50 ms gate not); 3-14 of 32 frames dropped per open (gpud dc present avg 24.9 ms)"
    note: "2026-11-19: judged in the motion package's board cycle (launcher paging with the rubber band, TASK-0354 P12). 2026-11-19 blocked: paging costs no raster (TASK-0354 P12), but tap to frame p50 58 ms > 50 and every open drops 3-14 of 32 frames: the GPU glass/blur passes (TASK-0351 P6) and the open path, not paging. Unblocks with TASK-0351 P6. 2026-11-19 (board--2026-11-19T15-50-37, after the damage origin): tap p50 48.5 ms over 70 opens (the 50 ms half holds); every open still drops frames — 105 ok / 16 warn / 58 FAIL windows, transitions show 8-9 of 17 frames: one glass write per dropped frame (~15 ms each, glass_w 11-17 per 32 frames). Stays blocked on TASK-0351 P6. 2026-11-20 package A: the gate was red, not blocked — cycle 4 panel taps 61-83 ms (gate <= 50); the trace names app-host's in>disp one blocking wait 26-36 ms as the tap's largest hop. 2026-11-20 board--2026-11-20T12-38-17 (W1 credit at drawn): app-host credit wait on tap frames p50 30.6 → 0 ms, in>disp 26-36 → 2.5-3 ms, slow app frames 19 → 5 bare / 85 → 16 loaded, 0 FAIL present drawn; panel taps 61-83 → 50-84 ms (gate <= 50 not yet): the tap now waits in windowd (owed>scene p50 30-32 ms for its one slot) behind frames whose GPU time passes a period (sub>gpu p50 17-21 ms on slow frames). 2026-11-20 board--2026-11-20T15-34-30 (D2 desktop never raised, D1 pieces, D3 rest band): panel taps 50-84 → 46-57 ms (six of seven panel kinds <= 50, one at 57), fullscreen launcher 99 → 88 ms; drops 9.9 → 6.5 %; operator: 'fühlt sich alles gut an'. Gate (<= 50 ms and 0 of 32 dropped on every open) not reached in full. 2026-11-21 board--2026-11-21T07-37-47 (B1: store reuse, bands on 8 harts, RFC-0121 ink): fullscreen launcher tap 88 -> 49 ms; panel taps 40-57 ms (mixed); drops 6.5 -> 7.8 %. Cause of the drops found: the desktop's ink flipped to whole at every panel's press and exit (a press/fade counted as a moving base box; an exit ghost without a box) — 144 body ink changes, each a full-screen structure change; fixed (scene_raster stays_inside, base_ink motion-held nodes), proof in B2. 2026-11-22 board--2026-11-22T11-03-14 (B2: ink fix, reuse fix, P19-e): every panel tap 31-45 ms, the fullscreen launcher 50 ms — the tap part of the gate (<= 50 ms) is met; the frame part (0 of 32 dropped on every open) is not: drops 8.0 % of 137 windows, nearly all on the loaded desktop (2 slow frames in the bare blocks)."
---

## The ask

Operator, 2026-11-03: the top bar and the launcher are very slow.

## Measured before the cut

P0, board 2026-11-03 (`board--2026-11-03T19-13-30`, `docs/board/measurements/2026-11-03-motion-input-frame-baseline/`),
the six panels, the launcher small and big, paging, windows opened and closed:

- tap to submit 77 ms median over 55 `ux input` windows (15 ok, 20 warn, 20 FAIL); the frame
  24 of 32 dropped (median), worst frame 102 ms median.
- **The dominant hop is gpud's frame**: `windowd: hops frame` `sent>acked` 90 ms mean (worst
  396 ms) against `owed>sent` 16 ms and the staging stage `staged>applied` 55 µs mean (worst
  8.8 ms). The input path in windowd is not the cost; the display's frame is — its owner is
  TASK-0351 (the passes, the per-pixel cost: 6 ns/px median over ~2.8 Mpx of slot area, 16
  layers). P1 is blocked on that cut.
- The shell's own transition frames cost 77 ms each on the app side (TASK-0354 P1's line) — the
  second cost, removed by TASK-0354 P2.
- Not separable from the lines: which panel or launcher size a tap opened. The ux lines are
  windows of 16 inputs, not taps; a per-control attribution needs windowd's motion record
  (TASK-0354 P2).

Before the cycle the suspects were the Control Center's full-screen glass re-blurred on any damage
(TASK-0335 context), the shell's CPU raster per transition frame, the pager glide on ScrollMomentum X.

## Three lenses (architecture-review, 2026-11-03)

- **Scope** — P0 is measurement only, from the lines that exist (`windowd: hops frame`, `gpud: ux
  frame`, `windowd: ux input`, `apphost: ux launch`), in the one board cycle shared with TASK-0351
  P0 and TASK-0354 P1. P1 cuts the hop P0 names, in that hop's owner: the shell's DSL (`userspace/
  apps/desktop-shell`), app-host's raster, windowd's compose, or gpud's passes. Out: a new mechanism
  before P0 names the hop; the motion mechanics (TASK-0354); the GPU's per-pixel cost (TASK-0351);
  the panels' pixels (TASK-0312, TASK-0313 stay the fidelity contract).
- **Invariant** — every hop is stamped from the kernel's clock on the receiving side, never from the
  app's claim; no new marker says `ok` before the budget is met (no-fake-green: a tap-to-frame line
  carries its verdict from `nexus-ux-budgets`); the numbers are read against
  `docs/dev/ui/foundations/quality/latency-reference.md` (tap <= 50 ms, 16.7 ms frame). Negative
  test: P0 adds none (it adds no mechanism); P1's cut carries its own `test_reject_*` by its owner.
- **Contract** — RFC-0108 (the budgets, the hops line format); RFC-0110 (the shown time the frame
  is judged by). No wire change in P0; P1 names its contract once the hop is known.

## Not doing here

- The general GPU per-pixel cost (TASK-0351); the motion model (TASK-0354); the design deltas
  (TASK-0353).

## Definition of Done
