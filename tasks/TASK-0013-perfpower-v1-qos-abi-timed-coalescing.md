---
title: TASK-0013 Performance & Power v1 (userspace): QoS ABI hints + timed service (timer coalescing)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on (SMP baseline): tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

The kernel scheduler already has QoS buckets (`QosClass`). To improve user experience and power we
want:

- a small user-visible QoS hint API (set/get) to influence scheduling policy,
- and a userspace timer coalescing service (`timed`) so frugal workloads batch wakeups.

This task is intentionally minimal and sits on top of stable kernel primitives.

Track alignment: QoS hints + timer coalescing are foundational for “device-class” services (GPU/NPU/Audio/Video)
to achieve low jitter and power-efficient defaults (see `tasks/TRACK-DRIVERS-ACCELERATORS.md`).

## Goal

Prove in QEMU:

- userspace can set/get its QoS hint via `nexus-abi` wrappers,
- `timed` provides bounded sleep/alarm APIs and coalesces wakeups based on QoS,
- and selftests demonstrate the expected behavior with deterministic markers.

## Non-Goals

- Perfect energy model or advanced QoS policies.
- Replacing all sleeps in the system (only a few call sites + selftest proof).
- Power governor, wake locks, or app standby (handled by `TASK-0236`/`TASK-0237`; `timed` provides coalescing only).

## Constraints / invariants (hard requirements)

- Deterministic markers; bounded timeouts; no busy loops.
- `timed` must not require kernel changes beyond the QoS syscall surface.

## Red flags / decision points

- **RED**:
  - Kernel QoS syscall surface does not exist yet; we must define it without breaking existing ABI.
- **YELLOW**:
  - Coalescing tests can become flaky if based on wall-clock deltas; prefer discrete batching markers
    rather than “measured RTT”.
- **GREEN**:
  - Kernel already tracks QoS buckets; mapping user hints to existing `QosClass` should be straightforward.

## Contract sources (single source of truth)

- `source/kernel/neuron/src/sched/mod.rs` QoS buckets (`QosClass`) as the initial mapping target.
- `scripts/qemu-test.sh` marker contract.

## Stop conditions (Definition of Done)

- `scripts/qemu-test.sh` includes and observes:
  - `timed: ready`
  - `SELFTEST: qos ok`
  - `SELFTEST: timed coalesce ok`

## Touched paths (allowlist)

- `source/libs/nexus-abi/` (qos syscall wrappers)
- `source/services/timed/` (new service)
- `userspace/` (client lib if needed, e.g. `userspace/nexus-time`)
- `source/services/execd/` and/or `source/init/nexus-init/` (apply QoS hints in a minimal way)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`
- `docs/kernel/` and `docs/services/`

## Plan (small PRs)

1. Define kernel syscall(s) for QoS set/get (minimal, stable error mapping).
2. Add `nexus-abi` wrappers and host-side tests for ABI mapping.
3. Implement `timed` service with coalescing windows based on QoS hints.
4. Wire a minimal call site (selftest + one service) to use `timed`.
5. Add selftest markers for QoS + coalescing.

## Acceptance criteria (behavioral)

- QoS syscalls work and are exercised in selftest.
- `timed` coalesces in a deterministic, testable way.
