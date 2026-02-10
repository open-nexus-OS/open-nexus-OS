---
title: TASK-0283 Kernel SMP prep v1: per-CPU ownership wrapper (`PerCpu<T>`) and `!Send` enforcement
status: Draft
owner: @kernel-team
created: 2026-01-09
links:
  - Vision: docs/agents/VISION.md
  - Rust SMP model: docs/architecture/16-rust-concurrency-model.md
  - SMP baseline: tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md
  - SMP hardening bridge: tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md
  - SMP parallelism policy: tasks/TASK-0277-kernel-smp-parallelism-policy-v1-deterministic.md
---

## Context

SMP correctness depends on preventing accidental cross-CPU mutable access. Rust can encode “CPU-local ownership” using a wrapper type that is not transferable (`!Send`) and can be made only accessible via an explicit “current CPU” accessor.

This directly supports the project goal: lower complexity and fewer concurrency bugs than C/C++ SMP implementations, and builds on the TASK-0012B hardening bridge.

## Goal

Introduce a kernel-internal `PerCpu<T>` abstraction that:

- makes CPU-local data **not sendable** (`!Send`) by construction,
- provides a safe accessor for “current CPU’s instance”,
- supports future bounded work stealing by explicit, audited synchronization.

## Non-Goals

- Implementing SMP itself (covered by TASK-0012 + TASK-0012B).
- Implementing lock-free structures (optional follow-up).

## Constraints / invariants (hard requirements)

- Must work in `no_std + alloc`.
- Must not add runtime overhead on hot scheduling paths.
- Must be explicit about any required `unsafe` and its invariants (ideally none).

## Security considerations

### Threat model

- **Cross-CPU mutable access**: race bugs causing scheduler corruption
- **Memory ordering bugs**: incorrect assumptions around visibility of per-CPU state

### Security invariants (MUST hold)

- Per-CPU state must not be mutated from other CPUs without explicit, reviewed synchronization
- Work stealing paths remain bounded and auditable

## Stop conditions (Definition of Done)

- `PerCpu<T>` exists and is used for at least:
  - scheduler state, or
  - IPI mailbox state
- Host tests validate invariants for the wrapper (where applicable).
- Documentation updated where the wrapper becomes normative.
- Adoption does not regress TASK-0012B bounded/deterministic scheduler + IPI proof behavior.

## Touched paths (allowlist)

- `source/kernel/neuron/` (per-CPU wrapper + adoption)
- `docs/architecture/16-rust-concurrency-model.md` (if normative API changes)
