# RFC-0025: IPC liveness hardening v1 - bounded retry, correlation, and deterministic timeout contract

- Status: In Review
- Owners: @runtime @kernel-team
- Created: 2026-02-16
- Last Updated: 2026-02-16
- Links:
  - Tasks: `tasks/TASK-0013B-ipc-liveness-hardening-bounded-retry-contract-v1.md` (execution + proof)
  - ADRs: `docs/adr/0025-qemu-smoke-proof-gating.md` (deterministic QEMU proof policy)
  - Related RFCs:
    - `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md`
    - `docs/rfcs/RFC-0022-kernel-smp-v1b-scheduler-hardening-contract.md`
    - `docs/rfcs/RFC-0023-qos-abi-timed-coalescing-contract-v1.md`

## Status at a Glance

- **Phase 0 (shared retry/correlation contract)**: ✅
- **Phase 1 (cross-service userspace migration)**: ✅
- **Phase 2 (kernel-aligned overload/liveness hardening)**: ✅

Definition:

- "Complete" means the contract is defined and the proof gates are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - bounded IPC retry semantics for OS-lite service routing and request/reply loops,
  - deterministic timeout/error behavior for retry-heavy IPC paths,
  - nonce-correlation retry handling contract (bounded mismatch handling, no unbounded drains),
  - ownership/newtype/Send-Sync/must_use expectations for liveness-sensitive IPC helpers.
- **This RFC does NOT own**:
  - redefining SMP architecture authority (`RFC-0021`/`RFC-0022` remain authoritative),
  - replacing service business logic contracts (for example observability, updates, policy semantics),
  - introducing remote/distributed observability semantics (`TASK-0038`/`TASK-0040`).

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- This RFC is implemented by `tasks/TASK-0013B-ipc-liveness-hardening-bounded-retry-contract-v1.md`.
- `RFC-0023` remains complete for QoS/timed v1 scope; this RFC is an explicit follow-on contract seed.

## Context

Multiple OS services currently carry local retry loops for routing and request/reply IPC with slightly different timeout and mismatch semantics. Under contention, this can lead to long-running retry behavior and inconsistent failure handling across services even when global harness timeouts eventually stop runs.

We need one shared, deterministic liveness contract:

- bounded retries,
- explicit deadline and attempt-budget behavior,
- bounded nonce-mismatch handling,
- stable reject/status behavior with auditable markers.

## Goals

- Define and enforce one bounded retry contract for IPC routing and request/reply loops.
- Remove ad-hoc unbounded retry/yield loops from high-risk service paths.
- Keep overload behavior deterministic and auditable (no fake progress).
- Preserve kernel ownership boundaries while hardening overload/liveness edges.

## Non-Goals

- Replacing service-level policy decisions or payload schemas.
- Replacing kernel scheduler model or introducing a second scheduling authority.
- Defining a cross-node retry protocol.

## Constraints / invariants (hard requirements)

- **Determinism**: retry loops are bounded and proof markers are deterministic.
- **No fake success**: `ready/ok` markers only after real behavior; timeout/failure markers reflect real failures.
- **Bounded resources**:
  - explicit deadline budget for retry loops,
  - explicit max-attempt/max-mismatch guard where needed,
  - no unbounded drain/yield loops.
- **Security floor**:
  - identity and authorization decisions remain bound to kernel-authenticated identity (`sender_service_id`) where applicable,
  - malformed/mismatched correlated replies fail closed.
- **Ownership/type/concurrency floor**:
  - retry/deadline/attempt budget boundaries use explicit types where practical,
  - retry outcomes are `#[must_use]`,
  - no new `unsafe impl Send/Sync` without explicit safety argument and tests.
- **Stubs policy**: no stub may claim liveness success.

## Proposed design

### Contract / interface (normative)

- Shared helper layer in `userspace/nexus-ipc` defines bounded retry primitives:
  - deadline-based retry,
  - explicit attempt budget checks,
  - bounded nonce-mismatch handling for correlated replies.
- Services use shared helpers instead of bespoke nested retry loops for routing/reply paths.
- Error model:
  - retry budget exhausted -> deterministic timeout status/error,
  - malformed correlation/mismatch over bound -> deterministic reject/error,
  - no silent fallback to infinite retry.

### Phases / milestones (contract-level)

- **Phase 0**: helper contract in `nexus-ipc` with tests for deadline/attempt/mismatch behavior.
- **Phase 1**: migrate high-risk services (`timed`, `metricsd`, `rngd`) then remaining hotspots.
- **Phase 2**: kernel-aligned hardening for overload/liveness checks in scheduler/syscall proof paths.

## Security considerations

### Threat model

- Retry-loop abuse causing CPU starvation/DoS under queue contention.
- Correlation desync via nonce mismatch flooding.
- Inconsistent timeout handling causing fail-open or hidden liveness regressions.

### Security invariants

- Retry loops terminate deterministically by deadline or explicit attempt budget.
- Correlation mismatches do not cause unbounded processing.
- Failure paths remain deny/fail-closed where policy/correlation are involved.

### DON'T DO

- Don't add unbounded retry/drain/yield loops.
- Don't use payload identity to bypass kernel-authenticated identity.
- Don't emit success markers for timeout paths.

### Mitigations

- Shared bounded retry primitives and correlation helpers.
- Deterministic timeout/reject markers and host tests (`test_reject_*`).
- Explicit attempt-budget and mismatch-budget handling in critical loops.

### Open risks

- Some legacy service paths may still use local loops during migration; phase gating must keep this explicit.
- Kernel-side secondary-hart trap-runtime hardening remains tied to `TASK-0247` completion scope.

## Failure model (normative)

- Deadline exhausted => timeout error/status (deterministic).
- Attempt budget exhausted => timeout/over-budget error/status (deterministic).
- Correlation mismatch beyond bounded budget => reject/fail (deterministic).
- No hidden infinite retry fallback.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nexus-ipc -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p timed -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
cd /home/jenning/open-nexus-OS && SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
cd /home/jenning/open-nexus-OS && SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

Observed on 2026-02-16:

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os` ✅
- `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` ✅
- `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` ✅
- `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=180s ./scripts/qemu-test.sh` ✅

### Deterministic markers (if applicable)

- Existing:
  - `timed: ready`
  - `SELFTEST: qos ok`
  - `SELFTEST: timed coalesce ok`
- New hardening markers (task-owned):
  - bounded retry timeout/reject markers for migrated paths.

## Alternatives considered

- Keep per-service retry loops and only patch individual bugs:
  - rejected; drifts semantics and repeats failures across services.
- Push all liveness policy into kernel:
  - rejected for this slice; service IPC behavior still needs deterministic userspace contracts.

## Open questions

- Should routing v1 become universally nonce-correlated now, or stay backward-compatible with mixed frames during migration?
- Which timeout status mapping should be stable per service family (uniform vs service-specific explicit map)?

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

- [x] **Phase 0**: bounded retry/correlation helper contract in `nexus-ipc` — proof: `cargo test -p nexus-ipc -- --nocapture`
- [x] **Phase 1**: cross-service migration from ad-hoc loops to shared bounded helpers — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- [x] **Phase 2**: kernel-aligned overload/liveness hardening and proofs — proof: `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=180s ./scripts/qemu-test.sh` (90s command remains runtime-sensitive)
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
