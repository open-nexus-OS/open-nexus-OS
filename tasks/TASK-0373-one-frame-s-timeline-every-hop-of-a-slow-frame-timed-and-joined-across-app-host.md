---
schema: 1
id: TASK-0373
title: "One frame's timeline: every hop of a slow frame timed and joined across app-host, windowd and gpud, its waits, yields and copies counted"
status: in_progress
owner: ["@ui"]
created: 2026-11-19
updated: 2026-11-20
subsystem: ui
kind: service
novelty: variant
estimate: 2
started: 2026-11-19
dates_source: recorded
goal:
  metric: "share of a slow frame's time named by a hop"
  baseline: "0 % — only window averages (RFC-0108 hops lines); a slow frame's hop, waits, yields and copies are guessed (board--2026-11-19T17-56-19: a regression read from sums, two wrong notes the same day)"
  target: ">= 95 % of every slow frame's time named by a hop on the board, joined across the three processes; record cost <= 50 us a frame"
  instrument: "nx hops <uart.log> --check (joined timelines, per-hop p50/p90 by class), the hops cost line"
consumer: TASK-0351/P6
phases:
  - id: P1
    story: "a slow frame prints its whole timeline once per process — app-host, windowd, gpud joined by the ids on the wire, every stamp from nsec, its waits, yields and bytes copied — and nx hops stitches a UART log into one timeline per frame"
    state: done
    gate: "host: nexus-ux-budgets timeline tests (slow rules, line cap, vocabulary, seq reuse), nx hops fixture tests; QEMU usb-visible: hops hist lines from all three, nx hops --check green; board: >= 95 % of each slow frame's time named, hops cost record_ns <= 50000"
    started: 2026-11-19
    done: 2026-11-20
    evidence: "build/logs/board--2026-11-20T10-20-46: hops one/hist/cost in app-host, windowd and gpud; nx hops joined 290 frames windowd+gpud, every one >= 95 % of its time named (p10 99.7 %, p50 99.9 %); record 4.3-7.3 us a frame (bound 50); --check's one finding = one gpud line cut by the console (instrument, not the system)"
    note: "2026-11-19 board board--2026-11-19T22-09-42: the instrument works on all three processes, cost 4-7 us a frame. Outliers = windowd stamped sent AFTER the non-blocking send (gpud stamped its receipt first): fixed, sent stamped before the send (compositor/runtime/gpud.rs). Decoder fixes: gap = span - hops (waits are inside the hops), windowd measured against gpud rx with its hops up to sent, no windowd gap without a gpud record. Done when the next cycle shows every joined frame >= 95 % named and --check green."
  - id: P2
    story: "the time a frame's thread was runnable but did not run is named: the kernel counts, per task, the time runnable and the time blocked, and the hop line shows it instead of a gap"
    state: planned
    gate: "RFC seed first (a kernel counter read by userspace); host: the counter's tests; board: nx hops names run-queue time per slow frame, the gap below 5 %"
    note: "2026-11-20: needs its RFC seed first (a new kernel count per task, CLAUDE.md workflow 2) — the next package decides when 2026-11-20 backlog: the RFC seed is its first step, not a blocker (the earlier 'blocked' only satisfied the old rule 8)."
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
