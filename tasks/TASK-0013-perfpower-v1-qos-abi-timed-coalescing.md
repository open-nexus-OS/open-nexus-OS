---
title: TASK-0013 Performance & Power v1 (userspace): QoS ABI hints + timed service (timer coalescing)
status: In Review
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - QoS/timed contract (this task): docs/rfcs/RFC-0023-qos-abi-timed-coalescing-contract-v1.md
  - Depends-on (SMP baseline): tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md
  - Depends-on (SMP hardening bridge): tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md
  - Depends-on (SMP hardening contract): docs/rfcs/RFC-0022-kernel-smp-v1b-scheduler-hardening-contract.md
  - Follow-up (RISC-V SMP runtime hardening): tasks/TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-virtioblkd-packagefs-selftests.md
  - Follow-up (SMP v2 affinity/shares): tasks/TASK-0042-smp-v2-affinity-qos-budgets-kernel-abi.md
  - Follow-up (PerCpu ownership wrapper): tasks/TASK-0283-kernel-percpu-ownership-wrapper-v1.md
  - Follow-up (SMP policy determinism): tasks/TASK-0277-kernel-smp-parallelism-policy-v1-deterministic.md
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
- Explicit ownership/newtype/concurrency contract for this task:
  - userspace ABI wrappers can carry integer wire values, but kernel-internal QoS state must be mapped to typed enums/newtypes,
  - no implicit cross-thread sharing of mutable scheduler/QoS state; preserve existing `Send/Sync` boundaries from TASK-0012B unless explicitly re-documented,
  - no `unsafe impl Send/Sync` for QoS/timer paths without an explicit safety argument in code and task notes.
- Must preserve TASK-0012B hardening invariants (bounded scheduler/SMP hot paths, deterministic SMP marker semantics, and single SMP authority).
- Must not weaken TASK-0012B trap-runtime ownership boundary: mutable trap-runtime kernel-handle access remains boot-hart-only until `TASK-0247` completes per-hart ownership handoff.

## Red flags / decision points

- **RED**:
  - none remaining in v1 closure scope.
- **YELLOW**:
  - Coalescing tests can become flaky if based on wall-clock deltas; keep marker-based proofs and bounded waits only.
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

- **QoS setter authorization must be explicit**:
  - self-targeted QoS set is allowed only for equal-or-lower class transitions,
  - self-targeted upward transitions are denied unless routed via privileged path,
  - setting QoS for other tasks requires privileged authority (`execd`/`policyd` path),
  - unauthorized escalation attempts are reject-tested and audited.
- **QoS bounds**: QoS class is validated as a typed enum; invalid inputs are hard-rejected with `-EINVAL` (no silent clamp)
- **Timer bounds**: Maximum number of live timers per task is `64` (prevent memory exhaustion)
- **Coalescing windows**: Deterministic QoS-class windows (`PerfBurst=0ns`, `Interactive=1ms`, `Normal=4ms`, `Idle=8ms`)
- **Priority preservation**: High-priority tasks cannot be indefinitely starved by lower-priority tasks
- **Audit trail**: QoS changes and timer registrations are logged for security analysis

### DON'T DO (explicit prohibitions)

- DON'T allow unauthorized QoS escalation (self-set must not bypass policy limits; other-pid set requires privilege)
- DON'T expose precise timer resolution to untrusted tasks (use coalescing to prevent timing side-channels)
- DON'T allow unbounded timer registrations (enforce per-task limits)
- DON'T use QoS class from payload strings (use kernel-provided task metadata)
- DON'T skip validation of QoS syscall arguments (validate enum values)
- DON'T log timer expiry times or coalescing windows in production (information leakage)

### Attack surface impact

- **Minimal**: direct QoS updates are constrained (`self` down/equal only; `other-pid` or upward transitions are privileged)
- **Controlled**: Timer coalescing is deterministic (no timing side-channels)
- **Bounded**: Timer registrations are per-task limited (no memory exhaustion)

### Mitigations

- **Explicit authorization split**:
  - self-target set path may exist but must be policy-clamped and reject-tested,
  - privileged path (`execd`/`policyd`) is required for other-pid QoS changes.
- **QoS validation**: Kernel validates QoS class enum values (reject invalid values with -EINVAL)
- **Timer limits**: `timed` enforces a hard cap of 64 live timers per task
- **Coalescing windows**: Deterministic coalescing windows (`PerfBurst=0ns`, `Interactive=1ms`, `Normal=4ms`, `Idle=8ms`)
- **Audit logging**: QoS/timer decisions emit deterministic audit markers (`QOS-AUDIT`, `timed: audit register ...`)
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

## Ownership / newtype / Send-Sync / must_use audit (TASK-0013 scope)

- `source/kernel/neuron/src/sched/mod.rs`
  - QoS wire mapping stays typed (`QosClass::from_u8`), no raw-int scheduler mutation paths.
  - Scheduler remains `!Send/!Sync`; no new unsafe thread-boundary claims.
- `source/kernel/neuron/src/syscall/api.rs`
  - syscall decoding uses explicit typed structs (`TaskQosArgsTyped`) and deterministic validation.
  - explicit authorization split (`self` down/equal vs privileged path) is enforced before mutation.
- `source/kernel/neuron/src/task/mod.rs`
  - task-local QoS state is encapsulated with typed getter/setter (no payload-string authority).
- `source/libs/nexus-abi/src/lib.rs`
  - userspace wrapper exposes typed QoS enum; invalid raw values map to explicit `AbiError::InvalidArgument`.
- `source/kernel/neuron/src/syscall/mod.rs`
  - syscall errors remain `#[must_use]`; new QoS path preserves explicit error handling.

## Stop conditions (Definition of Done)

- `scripts/qemu-test.sh` includes and observes:
  - `timed: ready`
  - `SELFTEST: qos ok`
  - `SELFTEST: timed coalesce ok`
- Host + compile gates remain green:
  - `cargo test --workspace` passes
  - `just diag-os` passes (if kernel syscall surface changes are introduced)
- Security-negative checks are present and green:
  - `test_reject_qos_set_unauthorized*`
  - `test_reject_invalid_qos_class*`
  - `test_reject_timer_registration_over_limit*`

## Touched paths (allowlist)

- `source/kernel/neuron/src/syscall/{mod.rs,api.rs}` (QoS syscall surface if this task defines set/get)
- `source/kernel/neuron/src/task/mod.rs` (task-side QoS metadata/validation wiring if required)
- `source/kernel/neuron/src/sched/mod.rs` (scheduler QoS hint wiring only if required by this task)
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
   - Authorization split must be explicit and testable (`self` path vs privileged `other-pid` path).
1. Add `nexus-abi` wrappers and host-side tests for ABI mapping.
   - Prefer strongly-typed enums for QoS classes; avoid exposing raw integers to userspace APIs where possible.
1. Implement `timed` service with coalescing windows based on QoS hints.
1. Wire a minimal call site (selftest + one service) to use `timed`.
1. Add selftest markers for QoS + coalescing.

## Acceptance criteria (behavioral)

- QoS syscalls work and are exercised in selftest.
- `timed` coalesces in a deterministic, testable way.

## Critical Delta Report (Soll vs Ist)

### Closed

- **QoS authority split**: self-path remains equal/lower only; privileged path for escalation/other-pid is now kernel service-identity bound (`execd`/`policyd`), not cap-slot heuristic.
- **Deterministic rejects**: invalid QoS wire values and invalid targets reject with `-EINVAL`; unauthorized sets reject with `-EPERM`.
- **Timed boundedness**: registration cap (`64`) and deterministic coalescing windows are active; over-limit rejects are deterministic.
- **Security audit floor**: QoS decisions and timed register decisions emit deterministic audit markers.
- **Exec path stability**: earlier deterministic `KPGF` and subsequent `ALLOC-FAIL` blockers were removed with runtime-verified fixes.

### Residual / follow-up expectations

- **Policy granularity** (follow-up): fine-grained delegation and richer QoS governance are not part of TASK-0013 v1; continue in `TASK-0042` policy/scheduler extension scope.
- **Trap/runtime ownership hardening** (follow-up): boot-hart-only mutable trap-runtime boundary remains as required until `TASK-0247`.
- **Audit centralization** (follow-up): current deterministic markers satisfy v1 proof/security floor; stricter sink-level consolidation remains a future hardening slice.

## Proof Evidence (closure rerun)

- `cargo test --workspace` ✅
- `just dep-gate` ✅
- `just diag-os` ✅
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os` ✅
- `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` ✅
- `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` ✅
