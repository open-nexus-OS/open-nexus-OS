# RFC-0026: IPC performance optimization v1 - deterministic control-plane reuse + zero-copy-aligned data paths

- Status: In Review
- Owners: @runtime @kernel-team
- Created: 2026-02-16
- Last Updated: 2026-02-12
- Links:
  - Tasks: `tasks/TASK-0013B-ipc-liveness-hardening-bounded-retry-contract-v1.md` (execution + proof)
  - Related RFCs:
    - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
    - `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`
    - `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md`
    - `docs/rfcs/RFC-0025-ipc-liveness-hardening-bounded-retry-contract-v1.md`

## Status at a Glance

- **Phase 0 (baseline + hotspot evidence)**: ✅
- **Phase 1 (control-plane reuse/caching)**: ✅
- **Phase 2 (data-plane zero-copy alignment + proof)**: ✅

Definition:

- "Complete" means the contract is defined and the proof gates are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - deterministic, minimal-invasive performance optimization rules for IPC control-plane hot paths,
  - reuse/caching contract for routed IPC clients and reply channels without changing kernel IPC ABI,
  - type-safety and error-handling constraints (ownership/newtypes/Send-Sync/must_use) for optimization work,
  - a data-plane split contract that keeps small control frames in IPC and uses existing zero-copy VMO paths for bulk payloads where applicable.
- **This RFC does NOT own**:
  - kernel IPC syscall ABI redesign or new IPC syscalls,
  - introduction of a centralized message hub/broker architecture,
  - distributed/softbus transport redesign,
  - non-deterministic benchmark-only tuning without marker/test evidence.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- This RFC is implemented as an extension of `TASK-0013B`.
- `RFC-0025` remains the liveness baseline contract; this RFC adds performance-oriented follow-up constraints on top of that baseline.

## Context

`TASK-0013B` converged service-local retry loops into bounded shared helpers and removed key liveness risks. This RFC extension adds deterministic control-plane reuse plus bounded data-plane guardrails and validates strict SMP=2 90s proofs with sequential QEMU discipline.

Current architecture already follows the intended model:

- capability-based local IPC (`RFC-0005`),
- nonce-correlated shared-inbox request/reply (`RFC-0019`),
- modern virtio-mmio deterministic proof floor (`RFC-0019` / testing policy),
- control-plane (small frames) vs data-plane (VMO/filebuffer for bulk) split.

The missing piece is deterministic control-plane overhead reduction without architecture drift.

## Goals

- Reduce avoidable control-plane overhead in hot paths (repeated route/setup/reply wiring work).
- Improve SMP=2 timeout headroom toward the 90s gate without changing authority boundaries.
- Keep optimizations deterministic, bounded, and security-preserving.
- Preserve and extend Rust hardening standards (newtypes, ownership, `Send/Sync` clarity, `#[must_use]` outcomes).

## Non-Goals

- Lock-free rewrites as a primary objective.
- Global message-hub introduction.
- Replacing existing IPC/capability semantics.
- Sacrificing determinism for best-effort throughput.

## Constraints / invariants (hard requirements)

- **Determinism**: no unbounded loops; stable marker ordering; bounded retries/deadlines.
- **No fake success**: markers only after real behavior.
- **Security floor**:
  - identity remains kernel-derived (`sender_service_id`),
  - policy remains deny-by-default via `policyd`,
  - CAP_MOVE lifecycle must remain leak-safe.
- **Ownership/type/concurrency floor**:
  - use explicit newtypes where boundary confusion is likely,
  - no new `unsafe impl Send/Sync` without explicit safety argument + tests,
  - preserve ownership clarity in cached client/channel state.
- **Error handling floor**:
  - use `#[must_use]` outcomes for optimization-relevant decision/error types where omission is risky,
  - no silent fallback from timeout to hidden success.
- **Modern MMIO floor**:
  - QEMU proofs must remain on modern virtio-mmio default policy.

## Proposed design

### Contract / interface (normative)

1. **Control-plane reuse contract**
   - Services SHOULD avoid repeated `new_for(...)` route/setup in hot paths.
   - Long-lived clients/reply endpoints are preferred; re-resolution should be bounded and explicit (on failure/rebind path).
   - Shared-inbox correlation semantics remain RFC-0019 compliant; no stale-drain correctness dependencies.

2. **Data-plane split contract**
   - Control-plane IPC frames remain small and bounded.
   - Bulk payload flows SHOULD use existing VMO/filebuffer paths where already supported by service contracts.
   - No new ad-hoc bulk inline payload growth in IPC hot paths.

3. **Type-safety contract**
   - Optimization state transitions (cache hit/miss/rebind/degraded) SHOULD use explicit enums/newtypes where practical.
   - Critical optimization outcomes must be handled explicitly (`#[must_use]` where omission risks hidden fallback).

4. **Failure contract**
   - Cache or reuse failures must degrade deterministically to bounded baseline behavior.
   - Rebind/retry policies must stay bounded and observable.

### Phases / milestones (contract-level)

- **Phase 0**: gather deterministic hotspot evidence and define bounded acceptance thresholds.
- **Phase 1**: implement control-plane reuse/caching optimizations in selected services and shared helpers.
- **Phase 2**: enforce data-plane split alignment and revalidate full proof matrix (including SMP=2 target gate).

## Security considerations

### Threat model

- Control-plane contention causing late-phase timeout pressure.
- Incorrect cache/reuse logic causing reply misassociation or stale authority use.
- Performance shortcuts introducing hidden fail-open behavior.

### Security invariants

- Capability checks and policy decisions remain authoritative and unchanged.
- Reply correlation remains nonce-validated on shared inboxes.
- Failure paths remain explicit and deterministic.

### DON'T DO

- Don't bypass `policyd` to save latency.
- Don't trust payload identity fields.
- Don't add unbounded cache growth or unbounded recovery loops.
- Don't trade away marker determinism for throughput.

### Mitigations

- bounded cache/rebind policies,
- explicit state/error outcomes,
- negative tests for stale/mismatch/degraded paths,
- marker and host-test proofs for deterministic fallback behavior.

### Open risks

- Host-load variability can still compress margin even when strict SMP=2 90s currently passes.
- Some service contracts may need additional zero-copy alignment work in follow-up tasks.
- Parallel QEMU smoke execution can invalidate evidence (harness-level contention); proof policy remains sequential-only.

## Failure model (normative)

- Route/setup cache miss -> bounded lookup/rebind; if unsuccessful, deterministic timeout/reject.
- Reuse state invalid -> explicit rebind attempt under bounded budget.
- Correlation mismatch beyond bounds -> reject/fail as defined by RFC-0019/RFC-0025.
- No silent fallback to unbounded retries.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nexus-ipc -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p timed -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test --workspace
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
cd /home/jenning/open-nexus-OS && SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
cd /home/jenning/open-nexus-OS && SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

### Phase-0 evidence and acceptance thresholds (implemented)

- Policy selected for this slice: `improvement_only` plus `prod_plus_harness`.
- Baseline and post-change use the same marker ladder and command floor.
- Measured strict SMP=2 90s wall-clock:
  - baseline: `1:24.93`
  - post-change: `1:23.77`
  - result: no regression, measurable improvement (`~1.16s`, about `1.4%`) on this host profile.
- Discipline rule:
  - QEMU smoke proofs are valid only when runs are sequential; parallel runs can invalidate evidence due to shared image/log contention.

### Deterministic markers (if applicable)

- Existing marker ladder must not regress.
- Optional new markers should be short, stable, and contract-focused (for example cache/rebind success/reject transitions).

## Alternatives considered

- **Lock-free-first rewrite**
  - rejected for this slice: high churn, weak evidence of dominant bottleneck.
- **Central message hub**
  - rejected: invasive architectural change, authority and latency risks.
- **Kernel ABI redesign for throughput**
  - rejected for v1 optimization slice: too invasive, higher drift risk.

## Open questions

- Which additional low-risk slices provide the best 90s headroom gains beyond this v1 pass without architecture drift?
- Should strict 90s remain mandatory for all environments, or be policy-gated by proven host class?

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

- [x] **Phase 0**: hotspot evidence + acceptance thresholds defined — proof: `cargo test -p nexus-ipc -- --nocapture`
- [x] **Phase 1**: control-plane reuse/caching slices implemented — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- [x] **Phase 2**: data-plane split alignment + SMP proof reruns — proof: `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
