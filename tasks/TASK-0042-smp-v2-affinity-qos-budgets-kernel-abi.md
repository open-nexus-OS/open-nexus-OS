---
title: TASK-0042 SMP v2: affinity hints + QoS CPU budgets (requires kernel ABI + execd wiring)
status: Draft
owner: @kernel-team @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - SMP baseline: tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md
  - SMP hardening bridge: tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md
  - QoS baseline: tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Follow-up (RISC-V SMP runtime hardening): tasks/TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-virtioblkd-packagefs-selftests.md
  - Drivers track (alignment): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Testing contract: scripts/qemu-test.sh
  - Unblocks: tasks/TRACK-DRIVERS-ACCELERATORS.md (QoS-aware driver scheduling, CPU affinity for latency-sensitive devices)
---

## Context

The prompt asks for “userspace-first” controls (affinity/pinning hints, CPU shares, latency/burst hints),
but today the kernel ABI does not expose any scheduler syscalls beyond basic yield/time. There is also no
`nexus_abi::sched::*` surface yet.

So this work **requires kernel changes** (new syscalls + per-task metadata) plus userspace wiring in `execd`.

## Goal

With SMP enabled (TASK-0012 + TASK-0012B), provide:

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
- Determinism: proofs must not rely on "exact timings"; prefer structural/ratio assertions.
- No `unwrap/expect`; no blanket `allow(dead_code)` in new userspace code.
- Preserve TASK-0012B hardening contract (bounded scheduler/SMP hot paths, deterministic resched evidence chain, no alternate SMP authority).
- Preserve TASK-0012B trap-runtime ownership boundary: mutable trap-runtime kernel-handle access remains boot-hart-only until `TASK-0247` introduces per-hart runtime authority.

## Security considerations

### Threat model

- **CPU affinity bypass**: Tasks attempting to escape affinity restrictions to access restricted CPUs
- **QoS shares manipulation**: Tasks attempting to increase their CPU shares to starve other tasks
- **Latency hint abuse**: Tasks setting aggressive latency hints to monopolize CPU time
- **Information leakage**: Affinity masks or CPU shares revealing system topology or other tasks' placement
- **Resource exhaustion**: Unbounded shares or burst hints causing scheduler overhead
- **Privilege escalation**: Tasks attempting to set affinity/shares for other tasks without authorization

### Security invariants (MUST hold)

All existing SMP security invariants from TASK-0012 remain unchanged, plus:

- **Affinity enforcement**: Kernel enforces affinity masks (tasks cannot run on disallowed CPUs)
- **Shares bounds**: CPU shares are bounded (min=1, max=1000) and validated
- **Privileged setting**: Only privileged services (e.g., `execd`) can set affinity/shares for other tasks
- **Self-modification allowed**: Tasks can set their own QoS/affinity (but not escalate beyond recipe limits)
- **Affinity clamping**: Affinity masks are clamped to online CPUs (invalid CPUs are ignored)
- **No information leakage**: Affinity queries do not reveal offline CPUs or system topology details

### DON'T DO (explicit prohibitions)

- DON'T allow tasks to set affinity for other tasks without explicit capability
- DON'T accept unbounded shares values (enforce min/max bounds)
- DON'T bypass affinity restrictions during work stealing (respect affinity masks)
- DON'T expose raw CPU topology via affinity APIs (abstract as "performance" vs "efficiency" cores)
- DON'T allow affinity changes for kernel threads (only userspace tasks)
- DON'T log affinity masks or shares in production (information leakage)
- DON'T use affinity for security isolation (use address spaces and capabilities instead)

### Attack surface impact

- **Minimal**: Affinity/shares syscalls are privileged (require capability or self-modification only)
- **Controlled**: Affinity masks are validated and clamped (no invalid CPU IDs)
- **Bounded**: Shares are bounded (prevent scheduler overhead from extreme values)

### Mitigations

- **Capability checks**: Affinity/shares syscalls require `CAP_SCHED_SETAFFINITY` capability for other tasks
- **Self-modification allowed**: Tasks can set their own affinity/shares (within recipe limits)
- **Bounds validation**: Shares clamped to [1, 1000], affinity masks clamped to online CPUs
- **Recipe limits**: `execd` enforces recipe-specified affinity/shares limits (cannot be exceeded)
- **Audit logging**: Affinity/shares changes logged to `policyd` for security analysis
- **Work stealing respects affinity**: Stolen tasks are only migrated to CPUs allowed by their affinity mask

### Affinity security policy

**Affinity assignment rules**:

1. **System services**: Can be pinned to specific CPUs (e.g., `compositord` on CPU 0)
   - Requires explicit `affinity=0x1` in recipe config
   - Gated by `policyd` (only allowed for trusted services)
2. **User apps**: Default affinity is "all CPUs" (no restrictions)
   - Apps can request "performance" or "efficiency" cores (abstract hints)
   - Cannot pin to specific CPU IDs (only abstract classes)
3. **Background tasks**: Can be restricted to "efficiency" cores
   - Explicitly set by `execd` based on QoS class

**Enforcement**:

- Kernel enforces affinity at scheduling time (only schedule on allowed CPUs)
- Work stealing respects affinity (no migration to disallowed CPUs)
- `policyd` gates affinity changes for other tasks (require capability)

### CPU shares security policy

**Shares assignment rules**:

1. **Default shares**: 100 (normal priority)
2. **Min shares**: 1 (lowest priority, for idle tasks)
3. **Max shares**: 1000 (highest priority, for system-critical services)
4. **Validation**: Kernel clamps shares to [1, 1000] range

**Enforcement**:

- Kernel uses shares as weight in round-robin scheduling (within QoS class)
- `policyd` gates high shares (>500) for non-system services
- `execd` applies shares from recipe configs (validated by `policyd`)

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
- Reuse TASK-0012B scheduler/SMP hardening decisions; do not fork a second scheduler path for affinity/shares.

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
