---
title: TASK-0012B Kernel SMP v1b: scheduler + SMP hardening (bounded queues, trap/IPI contract, CPU-ID fast path)
status: Draft
owner: @kernel-team
created: 2026-02-10
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - SMP baseline: tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md
  - SMP v1 contract: docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md
  - SMP v1b hardening contract (this task): docs/rfcs/RFC-0022-kernel-smp-v1b-scheduler-hardening-contract.md
  - Rust SMP model: docs/architecture/16-rust-concurrency-model.md
  - Kernel overview: docs/architecture/01-neuron-kernel.md
  - SMP policy: tasks/TASK-0277-kernel-smp-parallelism-policy-v1-deterministic.md
  - Follow-up (QoS/timed): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Follow-up (affinity/shares): tasks/TASK-0042-smp-v2-affinity-qos-budgets-kernel-abi.md
  - Follow-up (RISC-V extension): tasks/TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-virtioblkd-packagefs-selftests.md
  - Follow-up (PerCpu wrapper): tasks/TASK-0283-kernel-percpu-ownership-wrapper-v1.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

TASK-0012 established deterministic SMP v1 behavior and anti-fake IPI evidence. The next risk is not
"new features", but hardening scheduler/SMP internals so follow-up tasks (0013/0042/0247/0283) can
extend the system without re-opening basic correctness risks.

Current pain points:

- scheduler hot paths still use growth-capable queue structures,
- trap/IPI resched behavior is correct but not yet encapsulated as a small explicit contract surface,
- CPU-ID derivation uses a bounded scan in S-mode and should gain a clearer fast-path/fallback contract.

## Goal

Harden scheduler + SMP internals while preserving TASK-0012 externally proven behavior:

- keep SMP marker proofs and deterministic semantics unchanged,
- tighten bounded/no-surprise behavior on scheduler and IPI/trap paths,
- document and enforce ownership/synchronization contracts used by follow-up tasks.

## Non-Goals

- No new userspace scheduler ABI (owned by TASK-0013/TASK-0042).
- No affinity/shares policy implementation (owned by TASK-0042).
- No new RISC-V bring-up/storage authority (owned by TASK-0247).
- No "TASK-0012C" scope split in this slice; this task is the v1b hardening bridge.
- No hardware scheduler/MMIO scheduler acceleration.
- No async executor integration into kernel scheduler paths.

## Constraints / invariants (hard requirements)

- Preserve existing single-hart behavior when `SMP=1`.
- Preserve TASK-0012 SMP marker semantics and anti-fake causal chain:
  - `KSELFTEST: ipi counterfactual ok`
  - `KSELFTEST: ipi resched ok`
  - existing `test_reject_*` SMP markers.
- Keep SMP proof gates explicit (`REQUIRE_SMP=1` for SMP marker ladder).
- No fake success markers.
- No unbounded loops or unbounded queue growth in scheduler/IRQ/IPI hot paths.
- Avoid new `unsafe`; if unavoidable, document invariants at the callsite.
- Do not introduce a second SMP authority path outside TASK-0012/RFC-0021 contract.

## Red flags / decision points

- **RED (must decide in-task)**:
  - Queue capacity/backpressure contract in scheduler paths (reject vs defer semantics) must be explicit and tested.
- **YELLOW**:
  - Trap stack-top table synchronization style (`usize` table vs atomic table) must be chosen with trap/assembly compatibility in mind.
  - CPU-ID fast path must not assume `tp` ownership unless that invariant is proven and enforced.
- **GREEN**:
  - Existing IPI trap handling already provides a deterministic causal chain and can be hardened without changing marker semantics.

## Security considerations

### Threat model

- Cross-CPU state races in trap stack-top registration/reads.
- Resched/IPI bookkeeping drift causing false-positive progress signals.
- Scheduler resource exhaustion through unbounded queue growth.
- CPU-ID misidentification causing cross-CPU accounting/ack errors.

### Security invariants (MUST hold)

- Resched evidence chain remains causal and monotonic (`request -> send -> S_SOFT trap -> ack`).
- Cross-CPU visible SMP state uses explicit synchronization semantics.
- Scheduler hot paths remain bounded under load (no unbounded growth).
- No QoS inversion or task duplication/loss in bounded steal paths.

### DON'T DO (explicit prohibitions)

- DON'T bypass trap-side `S_SOFT` handling for IPI proof paths.
- DON'T add timing-based success criteria for SMP proofs.
- DON'T introduce unsynchronized shared mutable scheduler state.
- DON'T introduce hidden fallback SMP paths that skip existing proof gates.

### Attack surface impact

- Minimal to moderate (internal kernel hardening only; no new external ABI in this task).

### Mitigations

- Reuse existing deterministic marker contract and negative tests.
- Add focused host/kernel tests for bounded queue and synchronization decisions.
- Keep changes narrow to scheduler/SMP internals and validate with dual-mode QEMU proof ladder.

## Contract sources (single source of truth)

- `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md`
- `docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md`
- `source/kernel/neuron/src/sched/mod.rs`
- `source/kernel/neuron/src/core/smp.rs`
- `source/kernel/neuron/src/core/trap.rs`
- `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host)

- `cargo test --workspace`
- `just dep-gate`
- `just diag-os`

Required host/kernel checks include:

- bounded scheduler queue behavior under pressure (reject/backpressure path),
- no task loss/duplication in bounded steal behavior,
- CPU-ID fast-path/fallback behavior remains correct for boot and secondary harts.

### Proof (OS/QEMU)

- `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

Required markers remain green and unchanged in meaning:

- `KINIT: cpu1 online`
- `KSELFTEST: smp online ok`
- `KSELFTEST: ipi counterfactual ok`
- `KSELFTEST: ipi resched ok`
- `KSELFTEST: test_reject_invalid_ipi_target_cpu ok`
- `KSELFTEST: test_reject_offline_cpu_resched ok`
- `KSELFTEST: work stealing ok`
- `KSELFTEST: test_reject_steal_above_bound ok`
- `KSELFTEST: test_reject_steal_higher_qos ok`

## Touched paths (allowlist)

- `source/kernel/neuron/src/core/smp.rs`
- `source/kernel/neuron/src/core/trap.rs`
- `source/kernel/neuron/src/sched/mod.rs`
- `source/kernel/neuron/src/types.rs` (only if CPU/Hart ID helper contracts require narrow updates)
- `scripts/qemu-test.sh` (only if marker ordering/gating contract needs explicit sync)
- `docs/architecture/01-neuron-kernel.md`
- `docs/architecture/16-rust-concurrency-model.md`
- `docs/testing/index.md` (only if proof commands/marker expectations change)

## Plan (small PRs)

1. Scheduler hot-path hardening:
   - make queue growth/backpressure behavior explicit and bounded,
   - keep QoS ordering + bounded steal invariants unchanged.
2. Trap/IPI hardening:
   - encapsulate S_SOFT resched handling as explicit contract path,
   - preserve current anti-fake evidence semantics.
3. Trap-stack synchronization and CPU-ID fast path:
   - tighten synchronization contract for stack-top table access,
   - introduce/validate a faster CPU-ID path with deterministic fallback.
4. Proof sync:
   - keep marker contract stable,
   - refresh docs if any contract-level behavior changed.

## Acceptance criteria (behavioral)

- SMP behavior is unchanged at contract level, but internal scheduler/SMP paths are more bounded and auditable.
- Dual-mode SMP proof ladder remains green with unchanged marker semantics.
- Follow-up tasks can rely on TASK-0012B as the hardening baseline without introducing a second SMP authority.
