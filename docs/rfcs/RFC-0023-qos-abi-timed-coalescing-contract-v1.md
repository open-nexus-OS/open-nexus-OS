# RFC-0023: QoS ABI + timed coalescing contract v1

- Status: Implemented (v1)
- Owners: @runtime @kernel-team
- Created: 2026-02-11
- Last Updated: 2026-02-11
- Links:
  - Tasks: `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` (execution + proof)
  - ADRs: `docs/adr/0025-qemu-smoke-proof-gating.md` (QEMU proof policy)
  - Related RFCs:
    - `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md`
    - `docs/rfcs/RFC-0022-kernel-smp-v1b-scheduler-hardening-contract.md`

## Status at a Glance

- **Phase 0 (QoS syscall contract + typed mapping)**: ✅
- **Phase 1 (timed coalescing service contract)**: ✅
- **Phase 2 (proof sync + anti-drift hardening)**: ✅

Definition:

- "Complete" means the contract is defined and the proof gates are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - QoS hint syscall contract for set/get semantics and error model.
  - Timed service coalescing contract (deterministic, bounded behavior).
  - Authorization split for QoS updates (self path vs privileged other-pid path).
  - Ownership/newtype/Send-Sync constraints for this policy layer.
- **This RFC does NOT own**:
  - SMP baseline/hardening contract itself (`RFC-0021`/`RFC-0022` ownership).
  - Affinity/shares scheduler ABI (`TASK-0042` scope).
  - Per-hart trap-runtime ownership completion, NMI/FPU/stack-overflow policy (`TASK-0247` scope).
  - Replacing TASK-0012/TASK-0012B marker semantics.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- This RFC is implemented by `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`.

## Context

TASK-0012 and TASK-0012B stabilized scheduler/SMP internals and marker semantics. The next step is policy-layer behavior: minimal QoS hints plus deterministic timer coalescing, without reopening SMP authority boundaries or introducing implicit concurrency contracts.

## Goals

- Define a minimal, stable QoS set/get contract suitable for userspace wrappers.
- Define deterministic timer coalescing behavior for `timed` with bounded registration and bounded wakeup behavior.
- Keep ownership/type/concurrency constraints explicit so policy-layer work composes with hardened SMP internals.

## Non-Goals

- Affinity and shares policy (belongs to TASK-0042).
- Power governor and app standby policies (separate tasks).
- Any alternate scheduler/SMP authority path.

## Constraints / invariants (hard requirements)

- **Determinism**: proof markers and coalescing behavior must be deterministic; no timing-fluke success criteria.
- **No fake success**: no `ready/ok` markers unless real behavior occurred.
- **Bounded resources**: bounded timer registrations and bounded coalescing windows.
- **Security floor**:
  - QoS set authorization split is explicit and enforced.
  - Invalid/unauthorized QoS operations reject deterministically and are auditable.
  - Existing TASK-0012B bounded enqueue/trap/IPI/CPU-ID invariants remain intact.
- **Ownership floor**:
  - kernel-internal QoS state uses explicit typed enums/newtypes (no raw-int plumbing across internal boundaries),
  - no new implicit cross-thread mutable sharing in scheduler/timer policy paths.
- **Send/Sync floor**:
  - preserve existing `Send/Sync` boundaries from TASK-0012B unless explicitly re-documented,
  - no new `unsafe impl Send/Sync` without a written safety argument.
- **Stubs policy**: stubs must be labeled and non-authoritative.

## Proposed design

### Contract / interface (normative)

- QoS set/get contract:
  - syscall number: `SYSCALL_TASK_QOS = 15`.
  - wire args (stable): `(op, target_pid, qos_raw)`.
    - `op=0`: get self QoS (stable `u8` class value).
    - `op=1`: set QoS for `target_pid`.
  - setter authorization is explicit:
    - self-target set is allowed only for equal-or-lower transitions,
    - self-target upward transition requires privileged path,
    - other-pid set requires privileged authority (`execd`/`policyd` path).
  - deterministic errors:
    - invalid op/class/target rejects with `-EINVAL`,
    - unauthorized set rejects with `-EPERM`.
- `nexus-abi` wrappers expose strongly typed QoS class enums; kernel internals map to typed state.
- `timed` contract:
  - coalescing windows are class-based and deterministic:
    - `PerfBurst`: `0 ns`,
    - `Interactive`: `1_000_000 ns` (1ms),
    - `Normal`: `4_000_000 ns` (4ms),
    - `Idle`: `8_000_000 ns` (8ms).
  - timer registration bound: max `64` live timers per task.
  - deterministic rejects:
    - invalid timer arguments reject with `-EINVAL`,
    - over-limit registration rejects with `-ENOSPC`.
- Existing SMP marker semantics remain authoritative and unchanged.

### Phases / milestones (contract-level)

- **Phase 0**: QoS set/get contract + authorization split + typed mapping defined and tested.
- **Phase 1**: timed coalescing contract with bounded registration and deterministic markers.
- **Phase 2**: anti-drift proof sync against TASK-0012/TASK-0012B invariants.

## Security considerations

- **Threat model**:
  - unauthorized QoS escalation,
  - timer abuse for resource exhaustion,
  - timing side-channel amplification through precise wakeups.
- **Mitigations**:
  - explicit authorization split for QoS set paths,
  - deterministic validation and reject paths (`test_reject_*`),
  - bounded timer limits and deterministic coalescing windows,
  - audit trail for policy decisions.
- **Open risks**:
  - policy granularity for privileged escalation delegation (`execd` direct vs policyd-mediated) may need refinement in later slices.

## Failure model (normative)

- Invalid QoS class rejects deterministically (no silent clamp unless explicitly specified by task contract).
- Unauthorized QoS set rejects deterministically and is auditable.
- Invalid QoS op/target rejects deterministically (`-EINVAL`).
- Timer registration above bounds rejects deterministically (no unbounded queueing).
- No silent fallback to alternate scheduler authority.

## Critical Delta Report (Soll vs Ist)

### Closed deltas

- **QoS privileged authority path**: closed. `TASK_QOS_OP_SET` authorization is bound to kernel-derived `sender_service_id` (`execd`/`policyd`) instead of capability-slot heuristics.
- **QoS deterministic reject model**: closed. invalid class/target stays hard `-EINVAL`; unauthorized set stays `-EPERM`; self-escalation reject and other-pid unauthorized reject tests are present.
- **Timed contract and markers**: closed. deterministic windows + per-owner cap are implemented; `timed: ready` and `SELFTEST: timed coalesce ok` are observed.
- **Audit trail floor**: closed for v1. QoS decisions emit `QOS-AUDIT ...` and timed register decisions emit `timed: audit register ...`.
- **Proof ladder**: closed for v1 scope (host + OS + SMP=2 + SMP=1 reruns green).

### Residual risks and explicit non-goals

- **Policy granularity**: escalation delegation is still coarse (service-level privileged path), not per-capability/per-image policy intent.
- **Audit sink normalization**: v1 emits deterministic audit markers; later phases may route all policy/audit records through a stricter centralized sink model.
- **Future scheduler policy depth**: affinity/shares and richer QoS budgets remain follow-up scope (`TASK-0042`), not RFC-0023 v1.
- **IPC liveness hardening follow-up**: cross-service bounded retry/correlation convergence is tracked in `docs/rfcs/RFC-0025-ipc-liveness-hardening-bounded-retry-contract-v1.md` and `tasks/TASK-0013B-ipc-liveness-hardening-bounded-retry-contract-v1.md`.

## Security reject test matrix (normative)

- `test_reject_qos_set_unauthorized_self_escalation`:
  - expected: reject with permission error (`-EPERM`),
  - state assertion: caller QoS remains unchanged after reject.
- `test_reject_qos_set_unauthorized_other_pid`:
  - expected: reject with permission error (`-EPERM`),
  - state assertion: target QoS remains unchanged after reject.
- `test_reject_invalid_qos_class`:
  - expected: hard reject (`-EINVAL`, no clamp),
  - state assertion: caller/target QoS remains unchanged after reject.
- `test_reject_timer_registration_over_limit` (Phase 1/timed):
  - expected: reject with bounded-resource error (`-ENOSPC`),
  - state assertion: existing timer set remains intact; no partial enqueue.
- Selftest-client E2E negative assertions (marker-gated under `SELFTEST: qos ok` / `SELFTEST: timed coalesce ok`):
  - expected: unprivileged QoS self-escalation attempt rejects with permission error and leaves QoS unchanged,
  - expected: timed register with invalid QoS wire value rejects with `STATUS_INVALID_ARGS` and no timer allocation.

## Ownership / newtype / Send-Sync audit checklist

- QoS wire decode uses typed mapping (`QosClass::from_u8`) before scheduler/task mutation.
- Syscall decode is explicit (`TaskQosArgsTyped`) with deterministic op/arg validation.
- Kernel scheduler/task ownership boundaries remain explicit (`TaskTable` and `Scheduler` stay `!Send`/`!Sync`).
- No new `unsafe impl Send/Sync` introduced for QoS/timer paths.
- Error propagation remains explicit and deterministic (`#[must_use]` error enums + stable errno mapping).

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test --workspace
cd /home/jenning/open-nexus-OS && just diag-os
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
cd /home/jenning/open-nexus-OS && SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
cd /home/jenning/open-nexus-OS && SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

### Deterministic markers (if applicable)

- `timed: ready`
- `SELFTEST: qos ok`
- `SELFTEST: timed coalesce ok`
- Existing SMP anti-drift markers from TASK-0012/TASK-0012B remain green and unchanged.

## Alternatives considered

- Keep TASK-0013 under RFC-0022:
  - rejected because RFC-0022 owns SMP hardening internals, not QoS/timed policy contract.
- Implement timed-only without QoS contract:
  - rejected because coalescing policy requires explicit QoS source-of-truth.

## Resolved decisions (2026-02-11)

- Self-target QoS set is restricted to equal-or-lower transitions; any upward transition is privileged-path only.
- Invalid QoS input is hard-rejected with `-EINVAL` (deterministic, no silent clamp).

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- Scope boundaries are explicit; cross-RFC ownership is linked.
- Determinism + bounded resources are specified in Constraints section.
- Security invariants are stated (threat model, mitigations, DON'T DO).
- Proof strategy is concrete (not "we will test this later").
- If claiming stability: define ABI/on-wire format + versioning strategy.
- Stubs (if any) are explicitly labeled and non-authoritative.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: QoS set/get contract + authorization split + typed mapping — proof: `cargo test --workspace && just diag-os`
- [x] **Phase 1**: timed coalescing contract and markers — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- [x] **Phase 2**: anti-drift proof sync with SMP baseline/hardening contracts — proof: `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh && SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
