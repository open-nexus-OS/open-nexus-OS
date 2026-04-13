---
title: TASK-0288 Kernel runtime closure v1c: SMP/timer/IPI latency budgets + deterministic stress proofs
status: Draft
owner: @runtime @kernel-team
created: 2026-04-13
depends-on:
  - TASK-0012B
  - TASK-0247
  - TASK-0277
  - TASK-0283
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - SMP v1 baseline: tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md
  - SMP hardening bridge: tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md
  - RISC-V SMP/timer extension: tasks/TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-virtioblkd-packagefs-selftests.md
  - SMP policy determinism: tasks/TASK-0277-kernel-smp-parallelism-policy-v1-deterministic.md
  - Per-CPU ownership wrapper: tasks/TASK-0283-kernel-percpu-ownership-wrapper-v1.md
  - UI/kernel perf floor consumer: tasks/TASK-0054B-ui-v1a-kernel-ui-perf-floor-zero-copy-qos-hardening.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

The SMP/runtime path has the right direction, but "production-grade" needs more than bring-up
markers:

- latency-sensitive work must have bounded runtime behavior,
- timer/IPI paths must survive deterministic stress,
- and QEMU proofs must show that the kernel stays honest under pressure, not just at idle boot.

This task is the runtime closeout step, not a scheduler redesign.

## Goal

Close the runtime gap with explicit kernel latency-budget and stress-proof contracts for:

- wakeup/reschedule path,
- timer tick / deadline delivery,
- cross-CPU handoff and work stealing,
- and priority/QoS-sensitive trusted-service scenarios.

## Non-Goals

- Replacing the scheduler family.
- Chasing benchmark vanity numbers without stable proof.
- GPU/display driver work.
- Introducing nondeterministic perf tooling as the proof source.

## Constraints / invariants (hard requirements)

- **Deterministic stress**: fixed fixtures, bounded loops, stable marker names.
- **No hidden rescue loops**: runtime closure cannot rely on "yield until it passes".
- **Explainable budgets**: use coarse bounded budgets and counters, not fragile microbenchmark claims.
- **Architecture continuity**: extend the existing SMP/QoS path only.

## Red flags / decision points (track explicitly)

- **RED (budget semantics)**:
  - choose the small set of latency/cross-core counters we are willing to support long-term.
- **YELLOW (QEMU realism)**:
  - budgets must be framed as closure floors for this environment, not universal hardware claims.
- **GREEN (scope)**:
  - runtime closure is about boundedness and stress honesty first, absolute throughput second.

## Security considerations

### Threat model
- Runtime hot paths degraded by pathological cross-core ping-pong.
- IPI/timer abuse causing soft DoS.
- Trusted-service QoS hints accidentally becoming ambient privilege.

### Security invariants (MUST hold)
- Cross-core queues and mailboxes remain bounded.
- QoS/timer controls stay restricted to documented authorities.
- Stress proofs do not disable hardening to "get green".

### DON'T DO (explicit prohibitions)
- DON'T add debug-only scheduler bypasses.
- DON'T relax affinity or ownership invariants just to improve a stress result.
- DON'T claim consumer readiness from a single happy-path trace.

## Contract sources (single source of truth)

- SMP baseline/hardening: `TASK-0012`, `TASK-0012B`
- RISC-V timer/IPI extension: `TASK-0247`
- Parallelism policy: `TASK-0277`

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - kernel/runtime tests prove:
    - bounded reschedule and timer bookkeeping under fixed stress fixtures,
    - no runnable-set loss during work stealing / migration,
    - trusted-service QoS paths clamp and behave as documented.
- **Proof (OS/QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=210s ./scripts/qemu-test.sh`
  - required markers:
    - `KSELFTEST: runtime timer budget ok`
    - `KSELFTEST: runtime ipi budget ok`
    - `KSELFTEST: runtime stress ok`
    - `SELFTEST: ui runtime floor ok`

## Touched paths (allowlist)

- `source/kernel/neuron/src/sched/`
- `source/kernel/neuron/src/arch/riscv/`
- `source/kernel/neuron/src/core/`
- `source/kernel/neuron/src/task/`
- `source/services/execd/`
- `source/apps/selftest-client/`
- `docs/architecture/01-neuron-kernel.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. Define the runtime closure counters and budget semantics.
2. Add deterministic stress fixtures for timer/IPI/reschedule/migration.
3. Wire trusted-service QoS scenarios used by UI-critical services.
4. Add QEMU proofs and update runtime docs.

## Acceptance criteria (behavioral)

- Runtime closure is proven under deterministic stress, not inferred from bring-up success.
- Timer/IPI/work-stealing behavior remains bounded under load.
- UI-critical trusted-service paths can rely on an explicit runtime floor.

## Evidence (to paste into PR)

- QEMU: runtime budget / stress marker excerpt.
- Tests: kernel/runtime stress fixture summaries.
