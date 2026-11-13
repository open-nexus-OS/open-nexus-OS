---
schema: 1
id: TASK-0362
title: "Hardware floating point and vectors: the kernel keeps each thread's FP and vector state, and every userspace program is built for one hard-float target"
status: done
owner: ["@kernel-team", "@runtime"]
created: 2026-11-10
updated: 2026-11-13
subsystem: kernel
kind: service
novelty: first
estimate: 3
started: 2026-11-12
done: 2026-11-13
active_days: 2
dates_source: recorded
variance_note: "estimated 3 days, built in 2 with a fifth phase added on the way (P4, the first vector kernels in the painter, the operator's rule that a mechanism is built in where it is used); what took the time was not the state switch but two findings on the emulator: the kernel has to run with the units off and with one vector configuration, or eight emulated harts retranslate the kernel's paths per configuration and the lock budget goes red"
goal:
  metric: "soft-float userspace programs in the image; FP and vector registers kept across preemption and migration"
  baseline: "every program is built for riscv64imac (soft float) although the board's harts carry F, D and the vector extension; the kernel saves neither register set; the row painter costs 100-355 ns per row pixel (board--2026-11-10T14-55-42, apphost: hops motion paint_us over rows at 1920 px)"
  target: "0 soft-float userspace programs in the image; FP and vector registers intact over 10 000 preemptions with migrations on QEMU and the board; the SVG golden digest and every host golden unchanged"
  instrument: "the build-truth gate's target list; SELFTEST: fpsoak ok / vsoak ok; pinched svg digest; apphost: hops motion paint_us before and after"
  result: "0 soft-float userspace programs (36 hard float, 0 soft); FP and vector registers intact over 10 000 hand-overs with migrations on QEMU (1, 2, 8 harts) and the board (fpsoak ok, vsoak ok, 8 harts, 2 clusters, 32-byte vector registers); SVG golden digest and host goldens unchanged; the row painter on the board 100-355 -> about 16-72 ns per row pixel for the same motion layers (402x240: 123 662 -> 18 506 us on hard float -> 17 232 with vector kernels; 343x122: about 23 600 -> 3 760 -> 3 060); span scenes on the stock system: fill 3.2 -> 0.6 ns per pixel, translucent 53.5 -> 15.5"
consumer: TASK-0367
phases:
  - id: P0
    story: "what the harts offer and what soft float costs is measured: the ISA line of the tree on QEMU and the board, and the row painter and the SVG raster built soft, hard and with vectors on the stock system"
    state: done
    gate: "docs/board/measurements: the ISA lines and ns per pixel for the three builds of both rasters; the userspace baseline written as an ADR from these numbers"
    started: 2026-11-12
    done: 2026-11-12
    evidence: docs/board/measurements/2026-11-12-float-and-vectors
    note: "Gate met: docs/board/measurements/2026-11-12-float-and-vectors (ISA lines of board, stock tree and emulator; ns per pixel for three builds of both rasters; eleven digests equal), ADR-0099 accepted. Board confirms the tree's line under our kernel: isa from hart=0 f=1 d=1 v=1 (board--2026-11-12T15-31-27). Closes after the commit."
  - id: P1
    story: "a thread's floating-point registers are its own: saved when it leaves a hart having used them, restored before it runs again on any hart, and a thread that never used them costs nothing"
    state: done
    gate: "QEMU with 1, 2 and 8 harts and the board: SELFTEST: fpsoak ok (10 004 hand-overs per thread, 0 differing registers, 100 hops beyond one hart); KSELFTEST: fp state moved ok; KSELFTEST: fp state lazy ok (refused=0; threads = only those that used the registers); KSELFTEST: bkl budget ok unchanged"
    started: 2026-11-12
    done: 2026-11-12
    evidence: build/logs/board--2026-11-12T15-31-27
    note: "Built 2026-11-12. QEMU green on the final kernel: smp1--2026-11-12T15-09-52, smp--2026-11-12T15-13-17 (2 harts), smp--2026-11-12T15-15-51 (8 harts, bkl budget ok). Board dev-781235e6 (board--2026-11-12T15-31-27, 8 harts, 2 clusters): SELFTEST: fpsoak ok (rounds=10004 partner=10004 hops=100, 0 differing registers), fp state moved ok (cpu0->cpu1), fp state lazy ok (threads=2 saves=10083 loads=10082 refused=0), bkl budget ok (max_wait=3184us max_hold=1ms), ladder 112 rungs. The board judge is red on one line outside this phase: xhcid: FAIL (step=control cc=4) while the receiver's mouse interface is set up (seen before in board--2026-11-11T10-41-29; the mouse delivered events). Host: 12 fp_rule tests (test-kernel 292). Findings: the kernel runs with the unit off (lock budget red 5 of 5 on eight emulated harts before, green after; RFC-0119 alternatives); a pin places a thread only at its next wake (TASK-0363 P5)."
  - id: P2
    story: "the same for the vector registers, and a program asks the kernel what its hart offers instead of assuming it"
    state: done
    gate: "SELFTEST: vsoak ok on QEMU (1, 2, 8 harts) and the board; SELFTEST: hw probe ok ( on QEMU with vector_bytes=16 and on the board with vector_bytes=32, own_bytes equal (the width is each hart's own register, the tree says which harts have the unit); KSELFTEST: vector state lazy ok ( and moved ok (; KSELFTEST: bkl budget ok on eight emulated harts 3 of 3; test_reject_vector_use_without_the_extension, test_reject_a_word_that_contradicts_itself"
    started: 2026-11-12
    done: 2026-11-13
    evidence: build/logs/board--2026-11-13T09-35-00
    note: "proven 2026-11-13 on QEMU (1, 2, 8 harts: smp1--2026-11-12T18-34-32, smp--2026-11-12T18-42-31, smp--2026-11-12T18-45-01) and the board (board--2026-11-13T09-35-00: vsoak ok, hw probe ok vector_bytes=32 own_bytes=32 word=0x200000001f, vector state moved ok cpu4 to cpu1, lazy ok saves=10098 refused=0, bkl budget ok max_wait=4995us, ladder 116); finding: the emulator translates code per vector configuration, so the kernel sets a hart's length, type and start to the reset ones at a vector thread's trap entry (lock budget on eight emulated harts red 2 of 2 without, green 3 of 3 with); vector registers are kept across syscalls; waits for the commit"
  - id: P3
    story: "every userspace program is built for one hard-float target and the soft-float target is gone from the build; the kernel stays integer-only"
    state: done
    gate: "check-image-budgets: float ABI 0 soft float; SELFTEST: svg local determinism ok and pinched svg ok (digest unchanged); QEMU lanes with 1, 2 and 8 harts green; board ladder complete; and it is used: on the board apphost: motion layer paint_us for the same layers at least 3x lower than soft float (402x240: 123 662 us in board--2026-11-12T15-31-27) and apphost: ux launch render_ms reported before and after for the same apps"
    started: 2026-11-12
    done: 2026-11-13
    evidence: build/logs/board--2026-11-13T09-35-00
    note: "proven 2026-11-13: 36 images hard float, 0 soft float; QEMU 1/2/8 harts green; board--2026-11-13T09-35-00: motion layer 402x240 123 662 -> 18 506 us (6.7x), 391x548 310 679 -> 45 992 (6.8x), 343x122 23 6xx -> 3 76x (6.3x), 41-row motion pass 74.3 -> 9.1 ms, ux launch render_ms 362-1142 -> 99-442 (2 of 6 inside the 300 ms budget, 0 before); the SVG icon raster has no line of its own in the board log (its share is in render_ms) — the earlier gate's 3x on it is not measured on the OS; a hart without F and D is no CPU, a boot hart without them ends the boot (nexus-fdt Topology::read requires, Error::Lacks); waits for the commit"
  - id: P4
    story: "the frames are painted with the vector unit: the painter's flat fills and source-over blends — what apps, motion layers and animations are drawn with — run a vector kernel on a hart that has one, chosen at run time from what the kernel reports, the scalar path stays the reference"
    state: done
    gate: "QEMU (16-byte registers) and board (32): SELFTEST: paint kernels ok (every vector kernel byte-equal to its scalar reference over random spans, digest printed, the same digest on both); apphost: paint kernels (vector=1 width=16 | 32); the measurement program with the kernels chosen at run time, on the board's stock system: scene fill <= 1 ns per pixel and scene cards <= 25 (3.2 and 46.9 hard-float scalar, 0.5 and 21.3 compiler-vectorised, P0), its seven painter digests equal P0's; our system: apphost: hops motion paint_us per row pixel named before and after; host goldens byte-equal"
    started: 2026-11-13
    done: 2026-11-13
    evidence: build/logs/board--2026-11-13T10-38-20
    note: "proven 2026-11-13 on QEMU and the board, waits for the commit. Built: crate nexus-paint-kernels (reference.rs the scalar form, riscv.rs the vector forms in assembly taking their step from vsetvli, proof.rs the run every machine does, choose() once per program, spans below 64 bytes stay scalar); nexus-scene-raster blend_span and set_span and app-host's three row grounds call it, app-host chooses at start from sched::offers(). QEMU 1, 2, 8 harts (smp1--2026-11-13T10-12-08, smp--2026-11-13T10-15-43, smp--2026-11-13T10-27-13, smp--2026-11-13T10-30-06; eight harts bkl budget 2 of 3, the red smp--2026-11-13T10-18-11 waited 12-22 ms already in early kernel boot before any user program - the host): SELFTEST: paint kernels ok (vector=1 width=16 spans=8192 digest=0x287e0815b292fd61) = the host's digest. Board board--2026-11-13T10-38-20 (image dev-1d8bead9): paint kernels ok (vector=1 width=32, the same digest), apphost: paint kernels (vector=1 width=32) six times, ladder 118 rungs, bkl budget ok max_wait=3595us. Stock system, the measurement program with kernels chosen at run time: fill 3.2 -> 0.6 ns per pixel (gate 1), cards 53.5 -> 15.5 (gate 25), seven scene digests equal P0's; one 1920-pixel blend 35.8 -> 1.1 ns per pixel. Our system, the same motion layers before -> after: 343x122 3 760 -> 3 060 us, 391x548 45 992 -> 38 054, 393x687 60 414 -> 50 587 (7-23 % less); launches inside 300 ms 3 of 6 (2 before). Findings (docs/board/measurements/2026-11-13-paint-kernels): a real layer's row is mostly per-pixel work the two kernels do not touch (TASK-0367 P0 measures the split); the scalar reference's blend is 14 % slower than the loop it replaced (cards 53.5 against 46.9; it paints spans under 16 pixels and machines without the unit) and the vector blend already wins at 16 pixels - both at TASK-0367 P4; open and not looked at: gpud ux frame FAIL 24 (20 before), windowd ux input FAIL 11 (12)."
---

<!-- `nx ledger new task` copies everything below this line; the frontmatter above shows the
     planning fields (RFC-0115). Write state with `nx ledger`, never by hand. -->

## The ask

Who asked, in their words, and the outcome they want.

## Measured before the cut

What the code and the system do today — numbers, log paths, the measurement folder. A plan
that names no measurement is a guess.

## Three lenses

- **Scope** — in: … · out: … (with where each "out" goes).
- **Invariant** — what must hold, and the `test_reject_*` that proves it.
- **Contract** — the RFC/ADR this implements or seeds.

## Not doing here

The scope firewall: what is deliberately left out, and which task or milestone step takes it.

## Definition of Done

- Every phase `done` with its evidence; the goal's target reached on the instrument.
- Host tests, the QEMU lanes and the board cycle the phases name are green.
- Security-relevant work has its `test_reject_*` and hardening markers
  (`docs/standards/SECURITY_STANDARDS.md`).
- Docs swept: CHANGELOG, affected docs, the RFC's status; `active_days` and, when the estimate
  missed, a `variance_note`.
