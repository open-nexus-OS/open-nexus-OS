# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` (**CLOSED IN THIS SLICE**)
- **handoff_target**: `.cursor/handoff/current.md` (always updated as the live entry-point)
- **handoff_archive**: `.cursor/handoff/archive/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md` (latest completed-task snapshot, present)
- **linked_contracts**:
  - `docs/rfcs/RFC-0023-qos-abi-timed-coalescing-contract-v1.md` (current TASK-0013 contract seed)
  - `docs/rfcs/RFC-0022-kernel-smp-v1b-scheduler-hardening-contract.md` (completed hardening bridge contract)
  - `docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md` (completed SMP v1 baseline contract)
  - `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` (execution SSOT, complete)
  - `tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md` (hardening bridge, complete)
  - `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` (next execution SSOT)
  - `tasks/TASK-0277-kernel-smp-parallelism-policy-v1-deterministic.md` (normative SMP policy)
  - `docs/architecture/01-neuron-kernel.md` (ownership split + scheduler model)
  - `docs/adr/0025-qemu-smoke-proof-gating.md` (deterministic QEMU proof policy)
  - `docs/dev/platform/qemu-virtio-mmio-modern.md` (modern MMIO determinism floor)
  - `scripts/qemu-test.sh` (canonical marker contract; SMP markers must be explicitly gated)
- **first_action**: Select and prep the next task; TASK-0013 no longer blocks.

## Start slice (now)
- **slice_name**: Post-TASK-0013 handoff — choose next executable task
- **target_file**: follow TASK-0013 touched-path allowlist only
- **must_cover**:
  - preserve TASK-0012 marker and anti-fake invariants (no regression in SMP ladder)
  - preserve TASK-0012B bounded enqueue + CPU-ID + trap/IPI hardening invariants
  - avoid parallel scheduler authority (TASK-0013 extends policy surface, not SMP authority)
  - keep mutable trap-runtime boundary unchanged (boot-hart-only until TASK-0247)
  - align TASK-0013 touched-path allowlist with its syscall plan (kernel paths included if kernel QoS syscalls are in-scope)
  - record TASK-0013 closure evidence and carry forward guardrails:
    - keep `timed`/QoS markers and deterministic behavior stable,
    - preserve explicit QoS authority binding (`execd`/`policyd`) and audit-trail behavior,
    - keep proof commands reusable for regression checks.

## Execution order
1. **TASK-0011B**: complete (phases 0→5, proofs green, archived handoff snapshot)
2. **TASK-0012**: complete (SMP baseline + anti-fake proof markers + `test_reject_*`)
3. **TASK-0012B**: complete (hardening bridge on top of TASK-0012)
4. **TASK-0013**: next (baseline now green)

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - TASK-0012 + TASK-0012B are complete and export deterministic SMP guarantees required by TASK-0013
- **best_system_solution**: YES
  - QoS/timed policy work now builds on hardened scheduler/SMP internals
- **scope_clear**: YES
  - transition from hardening bridge to policy slice is explicit and bounded
- **touched_paths_allowlist_present**: YES (needs sync with final TASK-0013 syscall plan)
  - TASK-0013 allowlist must include kernel syscall/task/scheduler paths if QoS syscalls are implemented in this task

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES (after TASK-0013 header sync)
  - TASK-0012B output remains prerequisite floor for TASK-0013/0042/0247/0283
- **security_considerations_complete**: YES (after TASK-0013 authorization semantics are explicit)
  - preserve per-CPU isolation, explicit synchronization, and bounded resource contracts in follow-up slices
  - keep QoS set authorization explicit (self vs privileged other-pid), plus reject tests

## Dependencies & blockers
- **blocked_by**: none (TASK-0013 closure complete)
- **prereqs_ready**: YES
  - ✅ TASK-0011B complete and archived (`.cursor/handoff/archive/TASK-0011B-kernel-rust-idioms-pre-smp.md`)
  - ✅ TASK-0012 complete (strict SMP markers + anti-fake counterfactual + `test_reject_*` negatives)
  - ✅ TASK-0012B complete (bounded enqueue + trap/IPI contract hardening + guarded CPU-ID hybrid path)
  - ✅ modern MMIO/determinism policy aligned with harness/docs contracts

## Decision
- **status**: GO (pick next task)
- **notes**:
  - Keep TASK-0013 slices small and deterministic.
  - Preserve TASK-0012/TASK-0012B SMP proof commands and marker semantics unless explicitly revised with synchronized contracts/docs.
