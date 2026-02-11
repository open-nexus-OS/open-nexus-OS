# RFC-0022: Kernel SMP v1b scheduler/SMP hardening contract

- Status: Done
- Owners: @kernel-team
- Created: 2026-02-10
- Last Updated: 2026-02-11
- Links:
  - Tasks: `tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md` (execution + proof)
  - ADRs: `docs/adr/0025-qemu-smoke-proof-gating.md` (QEMU proof policy)
  - Related RFCs:
    - `docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md`
    - `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md`

## Status at a Glance

- [x] **Phase 0 (bounded scheduler contract)**
- [x] **Phase 1 (trap/IPI hardening)**
- [x] **Phase 2 (CPU-ID fast path + proof sync)**

Definition:

- "Complete" means the contract is defined and the proof gates are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - Bounded scheduler queue/backpressure contract for SMP hot paths
  - Trap/IPI resched hardening contract without marker-semantics drift
  - CPU-ID fast-path/fallback contract and determinism requirements
- **This RFC does NOT own**:
  - New userspace scheduler ABI (TASK-0013 / TASK-0042 scope)
  - Affinity/shares policy design (TASK-0042 scope)
  - New RISC-V bring-up/storage authority semantics (TASK-0247 scope)
  - Replacing TASK-0012 markers with new success semantics

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- This RFC is implemented by `tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md`.

## Context

TASK-0012 closed SMP v1 baseline behavior with deterministic anti-fake IPI proofs. Follow-up work needs a hardening bridge so scheduler/SMP internals can be tightened without reopening baseline correctness or introducing a second SMP authority.

## Goals

- Make scheduler queue/backpressure behavior explicit, bounded, and testable.
- Harden trap/IPI contract paths while preserving existing TASK-0012 marker meaning.
- Define CPU-ID fast-path/fallback rules that remain deterministic in SMP and SMP=1.

## Non-Goals

- No new external scheduler ABI or policy surface.
- No timing-based success criteria for SMP proof gates.
- No hidden fallback SMP path outside the existing contract.

## Constraints / invariants (hard requirements)

- **Determinism**: SMP proof ladder remains deterministic in SMP=2 and SMP=1 parity runs.
- **No fake success**: existing `KSELFTEST`/`SELFTEST` marker semantics remain authoritative and unchanged.
- **Bounded resources**: no unbounded queue growth, unbounded retries, or unbounded per-tick steal behavior.
- **Security floor**:
  - cross-CPU state mutations use explicit synchronization semantics,
  - resched evidence remains causal (`request -> send -> S_SOFT trap -> ack`),
  - no task loss/duplication under bounded steal rules.
- **Trap runtime ownership floor**:
  - mutable trap-runtime kernel handles are boot-hart-only in v1b; secondary-hart trap paths must fail closed until a per-hart authority is introduced.
- **Stubs policy**: no stub path may emit authoritative success markers.

## Proposed design

### Contract / interface (normative)

- Scheduler queue operations in SMP hot paths must use explicit bounded behavior:
  - v1b decision: reject new enqueue with stable, deterministic failure behavior (no unbounded retry loops).
  - callsites must consume bounded-enqueue outcomes explicitly (`#[must_use]`), not silently discard them.
- Trap/IPI resched path is contractually fixed as:
  - request acceptance,
  - IPI send success,
  - S-mode software interrupt trap observation,
  - resched ack.
  - wake/resched outcomes are matched explicitly at callsites (`#[must_use]` propagation), not silently ignored.
  - mutable trap-runtime kernel-handle access is only valid on boot hart in v1b.
- CPU-ID selection must have:
  - an explicit fast path with proven invariant assumptions (`tp` hint),
  - a deterministic bounded fallback when assumptions do not hold.
  - v1b decision: use `tp` as advisory hint, validate against bounded invariants, then fall back to stack-range, then BOOT fallback.
- Existing marker contract from RFC-0021 remains the authoritative external behavior.

### Phases / milestones (contract-level)

- **Phase 0**: Bounded scheduler queue/backpressure contract is explicit and tested.
- **Phase 1**: Trap/IPI hardening preserves anti-fake causal evidence semantics.
- **Phase 2**: CPU-ID fast path + fallback contract is validated in SMP=2 and SMP=1 parity proofs.

## Security considerations

- **Threat model**:
  - cross-CPU races in shared scheduler/trap-adjacent state,
  - false progress via non-causal or timing-only IPI success claims,
  - queue exhaustion paths causing starvation or task loss.
- **Mitigations**:
  - explicit synchronization contracts for cross-CPU state,
  - deterministic marker-gated proofs requiring causal chain presence,
  - bounded queue/steal semantics with negative tests (`test_reject_*`).
  - boot-hart-only guard for mutable trap-runtime kernel-handle access.
- **Open risks**:
  - fast-path CPU-ID assumptions must be proven or downgraded to fallback.
  - full per-hart trap runtime authority, kernel stack-overflow detection, NMI safety policy, and FPU context policy are deferred to `TASK-0247`.

## Failure model (normative)

- Queue saturation must fail/defer explicitly and deterministically (no silent drop).
- Invalid/offline IPI targets must reject with existing negative marker proofs.
- SMP marker mismatch is a hard failure in gated proof runs (`REQUIRE_SMP=1`).
- No silent fallback to alternate SMP authority path.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test --workspace
cd /home/jenning/open-nexus-OS && just dep-gate
cd /home/jenning/open-nexus-OS && just diag-os
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
cd /home/jenning/open-nexus-OS && SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

### Deterministic markers (if applicable)

- `KINIT: cpu1 online`
- `KSELFTEST: smp online ok`
- `KSELFTEST: ipi counterfactual ok`
- `KSELFTEST: ipi resched ok`
- `KSELFTEST: test_reject_invalid_ipi_target_cpu ok`
- `KSELFTEST: test_reject_offline_cpu_resched ok`
- `KSELFTEST: work stealing ok`
- `KSELFTEST: test_reject_steal_above_bound ok`
- `KSELFTEST: test_reject_steal_higher_qos ok`

## Alternatives considered

- Keep hardening inside RFC-0021:
  - rejected because it would blur "SMP v1 baseline" and "v1b hardening bridge" ownership.
- Move all hardening to task-only docs:
  - rejected because follow-up tasks need a stable contract seed.

## Open questions

- Resolved (TASK-0012B):
  - Queue backpressure default is immediate deterministic reject on saturation.
  - CPU-ID fast path is hybrid: `tp` hint first, stack-range fallback second, BOOT fallback last.

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

- [x] **Phase 0**: bounded scheduler queue/backpressure contract is explicit + tested — proof: `cargo test --workspace`
- [x] **Phase 1**: trap/IPI hardening preserves anti-fake causal chain semantics — proof: `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [x] **Phase 2**: CPU-ID fast-path/fallback contract is deterministic in SMP and parity modes — proof: `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
- [x] `#[must_use]` outcome contracts (enqueue/wake/resched) are consumed explicitly at callsites.
- [x] Mutable trap-runtime kernel-handle access is boot-hart-only in v1b (secondary-hart trap paths fail closed).
