---
title: TASK-0277 Kernel SMP/Parallelism Policy v1 (deterministic, auditable)
status: Draft
owner: @kernel-team
created: 2025-12-30
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Kernel SMP bring-up: tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md
  - RISC-V HSM/IPI + per-hart timers (extension): tasks/TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-virtioblkd-packagefs-selftests.md
  - Kernel IPC/caps baseline: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - Keystone gates: tasks/TRACK-KEYSTONE-GATES.md
---

## Context

Kernel parallelism (SMP) is necessary for performance and responsiveness, but it is also the highest
debug-cost part of the system: races, memory ordering, IRQ/IPI interactions, and scheduler corner cases.

This policy is the kernel equivalent of “parallelism without ideology”:

- keep the kernel minimal and auditable,
- allow concurrency where it is required (SMP, per-CPU scheduling),
- keep determinism for proofs (host/QEMU) and for security-critical logic as much as practical,
- avoid clever lock-free experiments until the basic invariants are proven.

## Goal

Define the v1 rules that all SMP/parallel kernel work must follow (including `TASK-0012` and follow-ups):

1. **Ownership model** (per-CPU data and who is allowed to mutate what)
2. **Locking model** (lock hierarchy, IRQ rules, no allocation in hot paths)
3. **Deterministic proof strategy** (KSELFTEST markers that validate *results*, not timing)
4. **Bounded behavior** (no unbounded logs/queues under contention)

## Non-Goals

- A new scheduling algorithm or “perfect fairness” policy.
- Lock-free datastructure experiments in v1.
- Proving “bit-for-bit” determinism of kernel scheduling across all interleavings (not realistic).
  Instead: deterministic, bounded **selftests** proving correctness invariants.

## Policy (v1 rules)

### Rule 1 — Per-CPU ownership by default

- Prefer per-CPU structures (runqueues, stats) with single-writer ownership by the local CPU.
- Cross-CPU operations must use explicit, auditable primitives:
  - IPI resched signals,
  - explicit “steal” operations with clear lock boundaries.

### Rule 2 — No heap growth in hot paths

- SMP/irq/scheduler paths must not allocate.
- Preallocate bounded buffers/queues where needed, or use fixed-capacity structures.

### Rule 3 — Locking rules are explicit

- Define a lock hierarchy for scheduler/task/IPC/MM subsystems.
- Rule of thumb:
  - prefer short critical sections,
  - avoid nested locks where possible,
  - document any required nesting with order constraints.
- Any “take lock under IRQ disabled” must be documented and minimized.

### Rule 4 — Deterministic proofs validate invariants, not timing

KSELFTESTs must:

- assert “CPU online mask is correct”, “IPI resched path works”, “work stealing preserves runnable set”
- avoid relying on “it ran on CPU1 within X ms”
- use bounded loops and explicit yields
- emit stable markers:
  - `KSELFTEST: smp online ok`
  - `KSELFTEST: ipi resched ok`
  - `KSELFTEST: work stealing ok`

### Rule 5 — Bounded logging and telemetry

- All debug logging under SMP must be throttled and bounded.
- Prefer counters + occasional summarized markers over per-event prints.

### Rule 6 — Minimal surface area first

- Keep the first SMP bring-up minimal:
  - online secondary harts deterministically,
  - per-CPU runqueues,
  - minimal IPI resched,
  - simple steal policy.
- Defer advanced features (tickless idle, complex load balancing) until v1 proofs are stable.

## Stop conditions (Definition of Done)

Planning-only completion criteria:

- `TASK-0012` references this policy as normative.
- Kernel SMP changes in subsequent tasks explicitly declare:
  - ownership model,
  - lock order,
  - which KSELFTEST markers prove the invariant.
