# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: `tasks/TASK-0013B-ipc-liveness-hardening-bounded-retry-contract-v1.md` (**IN REVIEW**)
- **handoff_target**: `.cursor/handoff/current.md` (always updated as the live entry-point)
- **handoff_archive**: `.cursor/handoff/archive/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` (latest completed-task snapshot, present)
- **linked_contracts**:
  - `docs/rfcs/RFC-0025-ipc-liveness-hardening-bounded-retry-contract-v1.md` (seed contract)
  - `tasks/TASK-0013B-ipc-liveness-hardening-bounded-retry-contract-v1.md` (execution SSOT + stop conditions)
  - `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md` (nonce-correlation floor)
  - `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` (baseline contract completed; this task is hardening follow-up)
  - `tasks/TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-virtioblkd-packagefs-selftests.md` (kernel runtime hardening dependency context)
  - `scripts/qemu-test.sh` (marker contract; deterministic proof gate)
- **first_action**: review proof package and timeout-floor caveat for final closeout decision.

## Start slice (now)
- **slice_name**: TASK-0013B review/closeout
- **target_file**: follow TASK-0013B touched-path allowlist only
- **must_cover**:
  - bounded retry/deadline/mismatch behavior for routing + reply loops
  - preserve sender-identity and fail-closed correlation semantics
  - deterministic markers and bounded test loops/timeouts
  - preserve TASK-0013/TASK-0014 behavior and marker ladders (no regressions)

## Execution order
1. **TASK-0011B**: complete
2. **TASK-0012**: complete
3. **TASK-0012B**: complete
4. **TASK-0013**: complete
5. **TASK-0013B**: in review

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - prereqs for local observability v2 are done (`TASK-0006`, `TASK-0009`, `TASK-0013`, `RFC-0019`)
- **best_system_solution**: YES
  - local-first metrics/tracing baseline reduces risk before remote/cross-node expansion
- **scope_clear**: YES
  - remote/cross-node explicitly deferred to `TASK-0038` and `TASK-0040`
- **touched_paths_allowlist_present**: YES
  - task includes dedicated touched list for service/lib/selftest/docs/harness

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - task header now lists `TASK-0038/0040/0041/0143/0046`
- **security_considerations_complete**: YES
  - threat model, invariants, DON'T DO, mitigations, security proof and hardening markers are present

## Dependencies & blockers
- **blocked_by**: none
- **prereqs_ready**: YES
  - ✅ `TASK-0006` completed (logd sink)
  - ✅ `TASK-0009` completed (`/state`)
  - ✅ `TASK-0013` completed (timed producer + QoS baseline)
  - ✅ deterministic proof policy remains aligned (`scripts/qemu-test.sh`, modern MMIO floor)

## Decision
- **status**: REVIEWING (TASK-0013B in review)
- **notes**:
  - keep scope local and deterministic; avoid follow-up creep
  - maintain reject-first security posture with explicit negative tests
