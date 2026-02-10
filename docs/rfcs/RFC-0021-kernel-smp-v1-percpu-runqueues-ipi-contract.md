<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0021: Kernel SMP v1 contract (per-CPU runqueues + IPI resched)

- Status: Complete
- Owners: @kernel-team
- Created: 2026-02-10
- Last Updated: 2026-02-10
- Links:
  - Tasks: `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` (execution + proof)
  - Tasks: `tasks/TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-virtioblkd-packagefs-selftests.md` (RISC-V extension, not parallel authority)
  - Tasks: `tasks/TASK-0042-smp-v2-affinity-qos-budgets-kernel-abi.md` (post-baseline SMP policy extension)
  - ADRs: `docs/adr/0025-qemu-smoke-proof-gating.md` (deterministic QEMU proof policy)
  - Related RFCs:
    - `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md` (pre-SMP ownership/types contract seed)
    - `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md` (testing contract + deterministic gates)
    - `docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md` (marker/readiness discipline)

## Status at a Glance

- [x] **Phase 0 (SMP baseline contract + gate shape)**
- [x] **Phase 1 (Secondary boot + trap-stack hardening)**
- [x] **Phase 2 (Per-CPU runqueues + IPI resched + bounded steal proofs)**

Definition:

- "Complete" means the contract is defined and the proof gates are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a design seed/contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The SMP v1 baseline contract for boot, scheduler ownership, IPI resched, and bounded work stealing.
  - The authority boundary between TASK-0012 baseline and TASK-0247 RISC-V extension.
  - The deterministic proof shape for SMP marker-gated runs (SMP>=2) plus SMP=1 regression.
  - The hard carry-over requirement that multi-hart trap entry must no longer depend on a global kernel stack symbol.
- **This RFC does NOT own**:
  - QoS/affinity/shares ABI and policy surfaces (owned by TASK-0042 and related tasks).
  - Full RISC-V bring-up/storage integration scope (owned by TASK-0247).
  - Detailed implementation choreography/checklists (owned by TASK-0012 execution SSOT).

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- This RFC must remain aligned with task-level execution truth; contract drift is resolved in the RFC, proof/progress drift in the task.

## Context

After TASK-0011B completed pre-SMP ownership/type hardening, TASK-0012 is the first behavioral SMP slice. Without an explicit SMP contract seed:

- extension tasks can create parallel SMP authorities,
- boot/trap assumptions can remain ambiguous (single-hart leakage),
- and proof gates can become timing-sensitive or nondeterministic.

This RFC fixes those boundaries before broad SMP implementation.

## Goals

- Define a minimal, auditable SMP v1 contract for QEMU `virt`.
- Lock authority boundaries: TASK-0012 baseline first, TASK-0247 extends only.
- Make trap-stack hardening a normative completion requirement for multi-hart correctness.
- Keep SMP proofs deterministic and bounded.

## Non-Goals

- Defining a full fairness/load-balancing scheduler.
- Defining affinity/shares APIs (post-baseline).
- Replacing existing marker contracts with timing-based checks.

## Constraints / invariants (hard requirements)

- **Determinism**: marker proofs must be deterministic; no timing-fluke pass criteria.
- **No fake success**: no `ok`/`ready` marker unless behavior is real.
- **Bounded resources**: bounded retry loops, bounded stealing, bounded IPI queue/mailbox behavior.
- **Security floor**: per-CPU ownership by default; cross-CPU mutation only through explicit synchronization.
- **No silent fallback**: if SBI HSM boot path is unavailable, fail fast with explicit evidence and keep SMP gate red.
- **Platform floor**: green proof runs use modern virtio-mmio default; legacy mode is debug-only and non-authoritative for success.

## Proposed design

### Contract / interface (normative)

#### 1) Secondary-hart boot contract

- On QEMU `virt`, TASK-0012 baseline uses SBI HSM `hart_start` for harts `1..N-1`.
- If HSM support is unavailable, TASK-0012 remains blocked; no hidden alternate boot path.
- TASK-0247 may harden/extend this path but must not introduce a second SMP authority.

#### 2) Trap-stack hardening contract

- Multi-hart trap completeness requires per-hart kernel stack source for U-mode trap entry.
- Global `__stack_top` assumptions are not accepted as SMP-complete behavior.
- `sscratch` semantics must remain per-hart deterministic.

#### 3) Scheduler ownership contract

- Runqueue ownership is per-CPU by default (single writer: local CPU).
- Any cross-CPU mutable access requires explicit synchronization and documented boundary rationale.
- SMP identifiers (CPU/Hart IDs) use explicit typed wrappers/newtypes instead of raw integers.

#### 4) IPI resched contract

- Introduce minimal IPI resched signaling sufficient for scheduler wake/resched.
- Sender identity is hardware CPU/Hart identity (not user payload).
- Queue/mailbox behavior must be bounded and auditable.

#### 5) Work stealing contract

- Stealing is bounded per attempt and deterministic.
- Stealing must preserve scheduler invariants (no task loss/duplication, no QoS inversion contract breakage).
- No unbounded "drain until works" behavior.

#### 6) Proof-gating contract

- SMP markers are checked only in explicit SMP proof mode; default single-hart smoke semantics remain unchanged.
- SMP baseline proof requires both:
  - SMP>=2 marker-gated run (behavioral proof),
  - SMP=1 regression run (compatibility proof).

### Phases / milestones (contract-level)

- **Phase 0**: Contract frozen in TASK-0012 + RFC; authority boundaries explicit.
- **Phase 1**: Secondary-hart boot + trap-stack hardening proven.
- **Phase 2**: Per-CPU runqueues + IPI resched + bounded stealing proven with deterministic markers.

## Security considerations

- **Threat model**:
  - Cross-CPU data races from implicit shared mutation.
  - IPI spoofing/abuse paths.
  - Unbounded stealing/IPI pressure causing denial-of-service.
  - Trap context corruption from shared stack assumptions.
- **Mitigations**:
  - Per-CPU ownership by default + explicit synchronization boundaries.
  - Hardware identity-based IPI trust boundary.
  - Bounded queue/steal policies.
  - Per-hart trap-stack contract as a hard completion gate.
- **Open risks**:
  - Memory-ordering details and lock hierarchy must be implemented carefully in TASK-0012 slices.
  - Harness-side SMP marker gate wiring must avoid regressions in default smoke behavior.

## Failure model (normative)

- Required behavior on failure:
  - Missing/unsupported HSM path: explicit failure evidence; do not claim SMP online success.
  - Missing SMP marker in SMP mode: fail gate.
  - SMP=1 regression failure: fail gate.
- No silent fallback from SMP path to "looks fine on single-hart."

## Proof / validation strategy (required)

Canonical proofs (implemented by tasks):

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test --workspace
cd /home/jenning/open-nexus-OS && just diag-os
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
cd /home/jenning/open-nexus-OS && SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

### Deterministic markers (if applicable)

- `KINIT: cpu1 online` (and higher as configured)
- `KSELFTEST: smp online ok`
- `KSELFTEST: ipi counterfactual ok`
- `KSELFTEST: ipi resched ok`
- `KSELFTEST: test_reject_invalid_ipi_target_cpu ok`
- `KSELFTEST: test_reject_offline_cpu_resched ok`
- `KSELFTEST: work stealing ok`
- `KSELFTEST: test_reject_steal_above_bound ok`
- `KSELFTEST: test_reject_steal_higher_qos ok`

## Alternatives considered

- **Defer all SMP contracts to TASK-0247**: rejected; causes authority ambiguity and delayed risk discovery.
- **Single global runqueue with coarse lock as baseline**: rejected; weak ownership boundaries and poor SMP migration clarity.
- **Timing-based SMP "performance proof" markers**: rejected; nondeterministic and brittle.

## Open questions

- None for v1 baseline. Follow-up contract changes must land in new RFC/task slices.

## RFC Quality Guidelines (for authors)

When updating this RFC, ensure:

- Scope boundaries stay explicit and extension ownership remains linked.
- Determinism and bounded resources stay normative (not optional notes).
- Security invariants remain explicit and verifiable.
- Proof strategy remains concrete and reproducible.

---

## Implementation Checklist

This section tracks implementation progress. Update as phases complete.

- [x] **Phase 0**: TASK-0012 links RFC-0021 and anti-drift boundaries remain aligned — proof: `rg "RFC-0021|TASK-0247" tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md`
- [x] **Phase 1**: Secondary-hart boot + per-hart trap-stack source implemented — proof: `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [x] **Phase 2**: Per-CPU runqueues + IPI resched + bounded steal proofs green — proof: `SMP=2 REQUIRE_SMP=1 ...` and `SMP=1 ...` runs
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`) where applicable.
