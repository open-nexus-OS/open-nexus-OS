---
title: TASK-0330 SMP v3: 8 harts + cluster topology from the FDT, per-hart PLIC/IRQ affinity, the IPC router and the address spaces out of the BKL, per-ASID shootdown
status: Draft (seeded 2026-09-21 by the hardware fast track — `tasks/IMPLEMENTATION-ORDER.md`; rewritten to end state at its block's P0)
owner: @kernel-team
created: 2026-09-21
depends-on: []
follow-up-tasks: []
links:
  - Execution order (the lane this belongs to): tasks/IMPLEMENTATION-ORDER.md
  - Playbook: CLAUDE.md
  - Lock classes and budgets: docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md, source/kernel/neuron/src/core/trap/budgets.rs
  - Per-hart EDT: docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md
  - Scheduler half: tasks/TASK-0306-ui-responsiveness-input-integrity-smp-rendezvous.md (Phase 3)
  - IPC fastpath numbers to hold: docs/rfcs/RFC-0096-ipc-performance-contract-v2-call-reply-recv-fastpath.md
  - Gate consumer: tasks/TRACK-TIME-AS-RESOURCE.md
  - RFC/ADR seeds (at P0): RFC-0102 SMP lock decomposition, ADR-0072 cluster-aware spreading, ADR-0073 BKL split order (from R9)
---

## Origin

No ledger carried the remaining SMP work: 0012/0012B/0042/0247/0277/0283/0288 are Done, `TASK-0306` Phase 3 (BKL hold) is the scheduler half and stays its own ledger (S2). Minted for target picture S; B/C parts seeded at P0.

## Context

`MAX_CPUS = 4`, per-hart runqueues live inside ONE big kernel lock (`core/trap/runtime.rs`) covering scheduler, tasks, IPC router, address spaces, timers, waitsets and fences; TLB shootdown is a full flush on every hart; PLIC delivery is boot-hart only; the display chain is pinned to cpu0. The board has 8 harts in two clusters.

## Goal

S1 (this ledger): `MAX_CPUS` 8 with the cluster topology from the FDT `cpu-map`, PLIC contexts + IRQ affinity on every hart (`KSELFTEST: smp online ok harts=8`, IRQs served off hart0). S3 (`0330B`): IPC router + waitsets out of the BKL (`ipc_call`/`reply_recv` fastpath lock-free on the hit path; the `TRACK-TIME-AS-RESOURCE` gate turns green). S4 (`0330C`): address spaces out of the BKL with per-ASID targeted shootdown. Every step: the BKL budgets shrink monotonically and the ipc call budget holds.

## Non-Goals

The scheduler lock itself (`TASK-0306` Phase 3 = S2), the placement policy (`TASK-0042B` = S5), heterogeneous scheduling beyond cluster awareness.

## Packages (from the order file; the P0 rewrite fixes them)

- **S1** (0330) — 8 harts + topology from FDT, per-hart PLIC contexts, IRQ affinity. Gate: `ci-os-smp` at `-smp 8`; board 8 harts online.
- **S3** (0330B, at P0) — IPC router + waitsets off the BKL. Gate: RFC-0096 numbers per hart; time-as-resource gate green.
- **S4** (0330C, at P0) — address spaces off the BKL + per-ASID shootdown. Gate: shootdown-count gate (targeted, not full).

## Constraints / invariants (hard requirements)

- **No fake success**: no `*: ready` / `SELFTEST: * ok` markers unless the real behavior happened; a
  human-visible board check is an operator-acked `board-visual:` marker, never prose.
- **The FDT is the one hardware truth**: no address, IRQ, frequency or hart count outside the parser.
- **Firmware blobs** only under `resources/firmware/<device>/` with provenance + license and a gate.
- **Vendor kernel code is reference only**; openly licensed userspace driver code may be ported.
- **Rust hygiene**: no `unwrap`/`expect` on untrusted input; `forbid(unsafe_code)` in userspace crates
  except the one documented MMIO/DMA seam per driver.

## Red flags / decision points

- **RED**: the measurements named in the order file (R-items) are done BEFORE the end-state rewrite.
- **YELLOW**: —
- **GREEN**: —

## Definition of Done

Filled at P0 from the order file's gates: host tests → QEMU profile → board lane marker(s), old
mechanism deleted with a gate against its return, docs sweep (CHANGELOG, board, RFC/ADR status).
