# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: `tasks/TASK-0014-observability-v2-metrics-tracing.md` (**IN REVIEW**, implementation complete)
- **handoff_target**: `.cursor/handoff/current.md` (always updated as the live entry-point)
- **handoff_archive**: `.cursor/handoff/archive/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` (latest completed-task snapshot, present)
- **linked_contracts**:
  - `docs/rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md` (seed contract)
  - `tasks/TASK-0014-observability-v2-metrics-tracing.md` (execution SSOT + stop conditions)
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md` (bounded sink baseline)
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md` (`/state` persistence baseline)
  - `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md` (nonce-correlation floor)
  - `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` (timed producer baseline)
  - `tasks/TASK-0038-tracing-v2-cross-node-correlation.md` (follow-up: cross-node correlation)
  - `tasks/TASK-0040-remote-observability-v1-scrape-over-dsoftbus.md` (follow-up: remote pipeline)
  - `scripts/qemu-test.sh` (marker contract; deterministic proof gate)
- **first_action**: review/closure decision for `TASK-0014`; select follow-up task only after explicit closure command.

## Start slice (now)
- **slice_name**: TASK-0014 review hardening + doc synchronization
- **target_file**: follow TASK-0014 touched-path allowlist only
- **must_cover**:
  - keep kernel untouched; userspace only
  - bounded cardinality/spans/payloads with deterministic rejects
  - logd export path as primary proof, not only in-memory checks
  - deterministic markers and bounded test loops/timeouts
  - preserve TASK-0013 behavior/markers (no regressions in timed/qos proofs)

## Execution order
1. **TASK-0011B**: complete
2. **TASK-0012**: complete
3. **TASK-0012B**: complete
4. **TASK-0013**: complete
5. **TASK-0014**: in review

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
- **status**: HOLD (wait for explicit TASK-0014 closure command before selecting next implementation task)
- **notes**:
  - keep scope local and deterministic; avoid follow-up creep
  - maintain reject-first security posture with explicit negative tests
