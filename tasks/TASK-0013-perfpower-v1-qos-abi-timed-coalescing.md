---
title: TASK-0013 Performance & Power v1 (userspace): QoS ABI hints + timed service (timer coalescing)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on (SMP baseline): tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md
  - Depends-on (SMP hardening bridge): tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md
  - Follow-up (RISC-V SMP runtime hardening): tasks/TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-virtioblkd-packagefs-selftests.md
  - Pre-SMP ownership/types contract (seed): docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md
  - Pre-SMP execution/proofs: tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

The kernel scheduler already has QoS buckets (`QosClass`). To improve user experience and power we
want:

- a small user-visible QoS hint API (set/get) to influence scheduling policy,
- and a userspace timer coalescing service (`timed`) so frugal workloads batch wakeups.

This task is intentionally minimal and sits on top of stable kernel primitives (TASK-0012 + TASK-0012B baseline).

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
- Any kernel changes required for QoS MUST follow the RFC-0020 contracts (TASK-0011B):
  - use explicit newtypes / enums internally (avoid “raw int” plumbing),
  - keep syscall ABI stable and explicit (errno mapping unchanged unless this task defines a new surface),
  - keep scheduler/QoS ownership and thread-boundary assumptions explicit (so QoS continues to compose with per-CPU runqueues from TASK-0012).
- Must preserve TASK-0012B hardening invariants (bounded scheduler/SMP hot paths, deterministic SMP marker semantics, and single SMP authority).
- Must not weaken TASK-0012B trap-runtime ownership boundary: mutable trap-runtime kernel-handle access remains boot-hart-only until `TASK-0247` completes per-hart ownership handoff.

## Red flags / decision points

- **RED**:
  - Kernel QoS syscall surface does not exist yet; we must define it without breaking existing ABI.
- **YELLOW**:
  - Coalescing tests can become flaky if based on wall-clock deltas; prefer discrete batching markers
    rather than "measured RTT".
- **GREEN**:
  - Kernel already tracks QoS buckets; mapping user hints to existing `QosClass` should be straightforward.

## Security considerations

### Threat model
- **QoS escalation**: Malicious tasks attempting to elevate their QoS class to gain CPU priority
- **Timer coalescing bypass**: Tasks attempting to bypass coalescing to get precise wakeups (side-channel timing attacks)
- **Resource exhaustion**: Unbounded timer registrations causing memory exhaustion in `timed` service
- **Denial of service**: High-priority tasks starving lower-priority tasks via QoS manipulation
- **Information leakage**: Timer coalescing windows revealing system load or other tasks' scheduling patterns

### Security invariants (MUST hold)

- **QoS escalation prevention**: Only privileged services (e.g., `execd`, `policyd`) can set QoS class (not arbitrary tasks)
- **QoS bounds**: QoS class is validated and clamped to valid range (Idle, Normal, Interactive, PerfBurst)
- **Timer bounds**: Maximum number of timers per task is bounded (prevent memory exhaustion)
- **Coalescing windows**: Coalescing windows are deterministic and do not leak timing information
- **Priority preservation**: High-priority tasks cannot be indefinitely starved by lower-priority tasks
- **Audit trail**: QoS changes and timer registrations are logged for security analysis

### DON'T DO (explicit prohibitions)

- DON'T allow arbitrary tasks to set their own QoS class (only privileged services)
- DON'T expose precise timer resolution to untrusted tasks (use coalescing to prevent timing side-channels)
- DON'T allow unbounded timer registrations (enforce per-task limits)
- DON'T use QoS class from payload strings (use kernel-provided task metadata)
- DON'T skip validation of QoS syscall arguments (validate enum values)
- DON'T log timer expiry times or coalescing windows in production (information leakage)

### Attack surface impact

- **Minimal**: QoS syscalls are privileged (only `execd` can set QoS for spawned tasks)
- **Controlled**: Timer coalescing is deterministic (no timing side-channels)
- **Bounded**: Timer registrations are per-task limited (no memory exhaustion)

### Mitigations

- **Privileged QoS setting**: Only `execd` (via recipe configs) can set QoS class for tasks
- **QoS validation**: Kernel validates QoS class enum values (reject invalid values with -EINVAL)
- **Timer limits**: `timed` enforces per-task timer limits (e.g., max 64 timers per task)
- **Coalescing windows**: Deterministic coalescing windows (based on QoS class, not system load)
- **Audit logging**: QoS changes logged to `policyd` for security analysis
- **Deny-by-default**: Tasks without explicit QoS setting default to Normal class

### QoS security policy

**QoS class assignment rules**:
1. **PerfBurst**: Reserved for system-critical services (compositor, audio mixer)
   - Requires explicit `qos=PerfBurst` in recipe config
   - Gated by `policyd` (only allowed for trusted services)
2. **Interactive**: User-facing apps (UI, input handlers)
   - Default for apps launched by user
   - Can be set by `execd` based on app manifest
3. **Normal**: Background services, non-interactive tasks
   - Default for services without explicit QoS setting
4. **Idle**: Maintenance tasks, indexing, telemetry
   - Explicitly set for low-priority background work

**Enforcement**:
- Kernel enforces QoS at scheduling time (priority ordering)
- `policyd` gates QoS escalation (PerfBurst requires explicit allow)
- `execd` applies QoS from recipe configs (validated by `policyd`)

## Contract sources (single source of truth)

- `source/kernel/neuron/src/sched/mod.rs` QoS buckets (`QosClass`) as the initial mapping target.
- `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md` (newtype + ownership + error envelope discipline)
- `tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md` (scheduler/SMP hardening baseline contract)
- `scripts/qemu-test.sh` marker contract.

## Stop conditions (Definition of Done)

- `scripts/qemu-test.sh` includes and observes:
  - `timed: ready`
  - `SELFTEST: qos ok`
  - `SELFTEST: timed coalesce ok`
- Host + compile gates remain green:
  - `cargo test --workspace` passes
  - `just diag-os` passes (if kernel syscall surface changes are introduced)

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
   - Kernel-internal implementation should use explicit types (newtypes/enums) per RFC-0020 (avoid raw ints).
2. Add `nexus-abi` wrappers and host-side tests for ABI mapping.
   - Prefer strongly-typed enums for QoS classes; avoid exposing raw integers to userspace APIs where possible.
3. Implement `timed` service with coalescing windows based on QoS hints.
4. Wire a minimal call site (selftest + one service) to use `timed`.
5. Add selftest markers for QoS + coalescing.

## Acceptance criteria (behavioral)

- QoS syscalls work and are exercised in selftest.
- `timed` coalesces in a deterministic, testable way.
