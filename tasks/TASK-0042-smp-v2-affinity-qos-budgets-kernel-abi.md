---
title: TASK-0042 SMP v2: affinity hints + QoS CPU budgets (requires kernel ABI + execd wiring)
status: Draft
owner: @kernel-team @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - SMP baseline: tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md
  - QoS baseline: tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Drivers track (alignment): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

The prompt asks for “userspace-first” controls (affinity/pinning hints, CPU shares, latency/burst hints),
but today the kernel ABI does not expose any scheduler syscalls beyond basic yield/time. There is also no
`nexus_abi::sched::*` surface yet.

So this work **requires kernel changes** (new syscalls + per-task metadata) plus userspace wiring in `execd`.

## Goal

With SMP enabled (TASK-0012), provide:

- per-task **affinity hint** (CPU mask / policy),
- per-task **CPU share** / latency / burst hints,
- deterministic proof in host tests and QEMU markers that:
  - hints are accepted and stored,
  - the scheduler respects them in a coarse, testable way (no perfect fairness required).

## Non-Goals

- Hard real-time scheduling guarantees.
- Complex load balancing policies.
- Exposing raw scheduler internals to userland.

## Constraints / invariants (hard requirements)

- ABI stability: introduce new syscalls without breaking existing IDs/semantics.
- Determinism: proofs must not rely on “exact timings”; prefer structural/ratio assertions.
- No `unwrap/expect`; no blanket `allow(dead_code)` in new userspace code.

## Required kernel changes (explicit)

- Add scheduler syscalls:
  - `sched_set_qos_class(pid, class)` (or set/get on current task)
  - `sched_set_affinity(pid, mask)` (hint; kernel may clamp to online CPUs)
  - `sched_set_shares(pid, shares)` (relative weight)
  - optional: `sched_set_latency_hint(pid, ms)` and `sched_set_burst_hint(pid, ms)`
- Store per-task fields in the kernel task table (and ensure they are copied/reset appropriately on spawn/exit).
- Update the SMP scheduler to consult these fields:
  - affinity as a placement constraint/hint,
  - shares as a weighted RR or coarse time-slice multiplier within a QoS class.

## Stop conditions (Definition of Done)

### Proof (Host)

- Kernel unit tests (or host-mode tests) for:
  - syscall argument validation,
  - deterministic clamping (invalid masks, invalid shares),
  - persistence of fields on the task structure.

### Proof (OS / QEMU)

- New markers in QEMU:
  - `execd: affinity set (svc=<...> mask=<...> policy=<...>)`
  - `execd: qos set (svc=<...> class=<...> shares=<...> lat=<...> burst=<...>)`
  - `SELFTEST: qos shares ratio ok`
  - `SELFTEST: affinity applied ok`

## Touched paths (allowlist)

- `source/kernel/neuron/src/sched/` and `source/kernel/neuron/src/task.rs` (store+apply hints)
- `source/kernel/neuron/src/syscall/{mod.rs,api.rs}` (new syscalls + stable IDs)
- `source/libs/nexus-abi/` (new wrappers)
- `source/services/execd/` (read recipes and apply to children)
- `recipes/sched/{affinity.toml,qos.toml}` (new config)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`
- `docs/perf/smp-v2.md`

## Plan (small PRs)

1. Kernel: define syscall IDs + implement setters (validate inputs, store fields).
2. Userspace: `nexus-abi` wrappers for the new syscalls.
3. Userspace: `execd` reads `recipes/sched/*.toml` and applies hints on spawn.
4. Kernel: make scheduler consult fields in a minimal, testable way.
5. Selftest: coarse ratio tests (shares) and placement assertions (affinity).

