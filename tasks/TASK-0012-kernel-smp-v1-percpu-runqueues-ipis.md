---
title: TASK-0012 Performance & Power v1 (kernel): SMP bring-up + per-CPU runqueues + IPIs (QEMU riscv virt)
status: Draft
owner: @kernel-team
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Kernel overview: docs/architecture/01-neuron-kernel.md
  - Depends-on (orientation): tasks/TASK-0011-kernel-simplification-phase-a.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

NEURON is currently effectively single-hart. To scale performance and enable power-aware policies we
need SMP bring-up on QEMU `virt` (RISC-V) with a minimal, debug-friendly scheduler model:

- secondary hart boot,
- per-CPU runqueues,
- IPIs for rescheduling,
- and a simple work-stealing policy.

Kernel changes are inherently high-debug-cost, so we gate each step with deterministic KSELFTEST
markers.

## Goal

Boot with SMP enabled (e.g. QEMU `-smp 2`) and prove:

- secondary CPU(s) come online,
- scheduling runs tasks across CPUs,
- IPI resched path works,
- and basic work stealing works when one queue is empty.

## Non-Goals

- QoS ABI and userland QoS policy (handled in TASK-0013).
- Interrupt-driven virtio; polling is fine.
- Advanced load balancing / fairness / starvation guarantees beyond simple stealing.

## Constraints / invariants (hard requirements)

- Preserve existing single-hart behavior when SMP=1.
- Deterministic markers for boot + selftests.
- Avoid unbounded logging and debug-only flood.

## Red flags / decision points

- **RED**:
  - Hart boot method: must choose a reliable mechanism on QEMU virt (SBI hart_start vs other). If
    unavailable in current environment, SMP bring-up is blocked.
- **YELLOW**:
  - Scheduler correctness under concurrency: keep locks minimal and auditable; prefer simple data
    structures first (per-CPU VecDeque + locks) before lock-free experiments.
- **GREEN**:
  - Kernel already has a QoS bucket scheduler (`QosClass`) and a deterministic tick model; SMP can
    extend this rather than redesigning scheduling from scratch.

## Contract sources (single source of truth)

- `docs/architecture/01-neuron-kernel.md` (scheduler overview + determinism)
- KSELFTEST marker contract (must be added/updated in kernel selftests)

## Stop conditions (Definition of Done)

- QEMU run with SMP>=2 produces:
  - `KINIT: cpu1 online` (and higher as configured)
  - `KSELFTEST: smp online ok`
  - `KSELFTEST: ipi resched ok`
  - `KSELFTEST: work stealing ok`
- Single-hart run (SMP=1) remains green with existing markers.

## Touched paths (allowlist)

- `source/kernel/neuron/src/**`
- `scripts/run-qemu-rv64.sh` (only if needed to parameterize `SMP`)
- `scripts/qemu-test.sh` (marker expectations for SMP runs, gated/optional)

## Plan (small PRs)

1. **CPU discovery + online mask**
   - Provide `cpu_current_id()` and `cpu_online_mask()`; log `KINIT: cpuN online` once per hart.

2. **Secondary hart boot**
   - Bring up harts 1..N-1 deterministically.

3. **IPI resched**
   - Implement a minimal S-mode IPI resched signal and handler; prove via selftest marker.

4. **Per-CPU runqueues**
   - Replace the single runqueue with per-CPU queues.

5. **Work stealing**
   - Simple round-robin steal when local queue empty; prove via selftest marker.

## Acceptance criteria (behavioral)

- SMP=2 reliably boots and emits the required KSELFTEST markers.
- No regressions for SMP=1.
