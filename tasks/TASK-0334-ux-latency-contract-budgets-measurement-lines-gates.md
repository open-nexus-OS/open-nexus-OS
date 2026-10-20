---
title: TASK-0334 The UX latency contract — budgets SSOT, measurement lines with a verdict, the lines on every ladder (package 0 before the compositor)
status: Done (2026-10-20, board cycle 49 — RFC-0108 Phase 0)
owner: @ui @runtime
created: 2026-10-20
depends-on: [TASK-0333 (6a ✅ 2026-10-20)]
follow-up-tasks: [the non-parking compositor (ADR-0075), the frame cost with glass (gpud), 6c the launch without a copy]
links:
  - RFC: docs/rfcs/RFC-0108-ux-latency-contract-instant-and-smooth-as-numbers.md
  - SSOT: source/libs/nexus-ux-budgets/src/lib.rs
  - Measurement: docs/board/measurements/2026-10-20-memory-lifecycle-baseline/README.md (cycles 47/48: the numbers that had no verdict)
  - Standard: docs/testing/os-markers.md ("Measurement lines and UX budgets")
---

## Context

- Cycle 47 on the desk board was green on every gate and felt like a slideshow, then froze
  twice (TASK-0333 "cycle 47"). The numbers existed (`gpu frame us` 66 ms of GPU wait at 4.3
  glass layers, the settings app 6.7 s to its first frame) but no line judged them: the gates
  measured correctness, not what a user feels.
- The operator's demand (2026-10-20): after the G/M/S lanes the board must feel like the
  reference class — instant open, 60 Hz smooth with glass and modern animation — and no run
  may forget that goal. So the goal becomes a contract with numbers, judged on every lane.
- Must not change: the kernel (budgets are no policy of its), the input wire format (the
  input's own stamp is the non-parking compositor's input contract), the existing lines'
  meaning (`gpu frame us`, the launch stages stay; the verdict lines join them).

## Goal

Every run reads its UX against budgets: three lines with a verdict on the board ladder
(`gpud: ux frame`, `windowd: ux input`, `apphost: ux launch`), one SSOT of the budgets, one
line shape; a red is a named red with its tracking reference, never a hidden one.

## Non-Goals

- Meeting the budgets (the follow-up packages do; their measurement folders read these lines).
- The UI alarm pill (RFC-0108 Phase 2) and the tap's feedback budget (Phase 1).

## Three lenses (architecture-review)

- **Scope**: `source/libs/nexus-ux-budgets` (new), `source/drivers/gpud/src/backend/gpu/desktop.rs`
  (the window's verdict line), `source/services/windowd/src/{ux_input.rs, compositor/runtime/
  {input.rs, present.rs, mod.rs}}`, `source/services/app-host/src/probe/{boot_env.rs, mod.rs}`,
  the proof manifest (`ui.toml`), `scripts/qemu-test.sh`, `scripts/board-test.sh`,
  `config/fail-marker-allow*.txt`, `docs/testing/os-markers.md`. Out: kernel, wire formats,
  metricsd, the shell.
- **Invariant**: `ok` only inside the budget — the verdict is `nexus_ux_budgets::judge`
  (host truth table, `test_reject_ok_for_a_launch_past_its_budget`,
  `test_reject_ok_for_an_input_past_its_budget`, `test_reject_ok_when_one_input_waited_past_the_red_line`);
  lines carry no identity.
- **Contract**: RFC-0108 extends the declarative-budget pattern of `core/trap/budgets.rs`
  (RFC-0096's IPC budget) to the user path; no boundary crossed, no ADR.

## Plan

- **P0 — the SSOT and the rule (host).** `nexus-ux-budgets`: the budgets, `Verdict`, `judge`,
  `FrameWindow`; windowd's `UxInput` window. Tests above.
- **P1 — the lines.** gpud: `gpud: ux frame <v> (dropped= of=32 max_us= budget_us=16667)`
  with its `gpu frame us` window; windowd: `windowd: ux input <v> (to_submit_us= of=16
  budget_us=50000)` from the first staged input to the submit that carries it; app-host:
  `apphost: ux launch <v> (first_frame_ms= budget_ms=300)` from its first instruction to the
  first frame's ack.
- **P2 — the ladders.** The three prefixes declared in `ui.toml`; `apphost: ux launch `
  REQUIRED on `headless`/`visible` (the greeter), all three on `board-visible`; the QEMU
  allow-list names `apphost: ux launch FAIL` and `windowd: ux input FAIL` (TCG timing is not
  the contract), the board allow-list names each `FAIL` with the package that tracks it.
- **P3 — the standard.** `docs/testing/os-markers.md`: the measurement-line shape, the
  verdict words, how a lane treats a red; CHANGELOG; IMPLEMENTATION-ORDER.
- **Proof**: host tests; `just test-os headless`, `visible`; board cycle 49 with the operator's
  series — the three lines read into the measurement folder with their verdicts (expected red
  on the board today: the frame with glass, the launch of settings/stash; named).

## Results

- **P0–P3 ✅ 2026-10-20 (board cycle 49, `board--2026-10-20T12-32-45`).** Host: the truth
  tables and `test_reject_*` (nexus-ux-budgets 4, windowd `ux_input` 2); `just check`, `just
  diag`, the structure gate green; headless and visible green with `apphost: ux launch FAIL
  (first_frame_ms=1275/1313 …)` under TCG, allow-listed by name; board `[PASS] board-visible:
  76 rungs, 8 operator markers` with every `FAIL` named in `config/fail-marker-allow-board.txt`.
- **The contract's first reading on the board** (`docs/board/measurements/2026-10-20-memory-lifecycle-baseline/cycle49-lines.txt`):
  - `gpud: ux frame FAIL` in every window: 14–32 of 32 frames dropped, `max_us` 92–315 ms;
    the `gpu frame us` split puts 95 % of a frame in `wait` (the GPU itself: 42–79 ms at
    2.4 Mpx of layer pixels per frame, with 1.6 as with 6.1 glass layers — the whole screen
    is redrawn every frame, not only the damage).
  - `windowd: ux input FAIL` in every window: the first staged input waits 0.6–5 s for the
    frame that carries it — the submit loop is throttled by the frames in flight, each
    50–300 ms on the GPU, so an input's wait is the GPU's backlog.
  - `apphost: ux launch`: calculator/chat/greeter/shell `warn` at 456–881 ms; settings `FAIL`
    8.1 s, stash 3.7 s.
  - One transient `hidrawd inputd send fail backpressure` (the input chain recovered this
    cycle; cycle 47's froze).
- **What the numbers say about the order:** the frame cost drives both the slideshow and the
  input's wait (the GPU's backlog); the non-parking compositor prevents the freeze. The
  operator decides the order with these numbers (the ledger's "next" lists the compositor
  first).
