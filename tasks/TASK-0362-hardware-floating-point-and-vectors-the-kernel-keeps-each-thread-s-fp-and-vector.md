---
schema: 1
id: TASK-0362
title: "Hardware floating point and vectors: the kernel keeps each thread's FP and vector state, and every userspace program is built for one hard-float target"
status: draft
owner: ["@kernel-team", "@runtime"]
created: 2026-11-10
updated: 2026-11-10
subsystem: kernel
kind: service
novelty: first
estimate: 3
goal:
  metric: "soft-float userspace programs in the image; FP and vector registers kept across preemption and migration"
  baseline: "every program is built for riscv64imac (soft float) although the board's harts carry F, D and the vector extension; the kernel saves neither register set; the row painter costs 100-355 ns per row pixel (board--2026-11-10T14-55-42, apphost: hops motion paint_us over rows at 1920 px)"
  target: "0 soft-float userspace programs in the image; FP and vector registers intact over 10 000 preemptions with migrations on QEMU and the board; the SVG golden digest and every host golden unchanged"
  instrument: "the build-truth gate's target list; SELFTEST: fpsoak ok / vsoak ok; pinched svg digest; apphost: hops motion paint_us before and after"
consumer: TASK-0367/P4
phases:
  - id: P0
    story: "what the harts offer and what soft float costs is measured: the ISA line of the tree on QEMU and the board, and the row painter and the SVG raster built soft, hard and with vectors on the stock system"
    state: planned
    gate: "docs/board/measurements: the ISA lines and ns per pixel for the three builds of both rasters; the userspace baseline written as an ADR from these numbers"
  - id: P1
    story: "a thread's floating-point registers are its own: saved when it leaves a hart having used them, restored before it runs again on any hart, and a thread that never used them costs nothing"
    state: planned
    gate: "QEMU smp and board: SELFTEST: fpsoak ok (10 000 preemptions, migrations above 0); KSELFTEST: fp state lazy ok (0 bytes saved for a thread that never used FP)"
  - id: P2
    story: "the same for the vector registers, and a program asks the kernel what its hart offers instead of assuming it"
    state: planned
    gate: "SELFTEST: vsoak ok on QEMU and the board; SELFTEST: hw probe ok (the board reports the vector extension and its register width from the tree); test_reject_vector_use_without_the_extension"
  - id: P3
    story: "every userspace program is built for one hard-float target and the soft-float target is gone from the build; the kernel stays integer-only"
    state: planned
    gate: "build-truth gate: 0 riscv64imac userspace artifacts; pinched svg digest and test-host goldens unchanged; headless, visible and smp lanes green; board-visible PASS"
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
