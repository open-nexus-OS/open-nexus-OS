# Next Task Preparation (Drift-Free)

<!--
CONTEXT
Preparation file for the next execution slice.
Update during wrap-up so a new session can start without drift.
-->

## Candidate next task
- **task**: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (`Draft`)
- **handoff_target**: `.cursor/handoff/current.md`
- **handoff_archive**: `.cursor/handoff/archive/TASK-0019-security-v2-userland-abi-syscall-filters.md`
- **linked_contracts**:
  - `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`
  - `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md`
  - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
  - `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
  - `tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md`
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
  - `tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md` (follow-on boundary)
  - `tasks/TASK-0188-kernel-sysfilter-v1-task-profiles-rate-buckets.md` (kernel follow-on boundary)
  - `docs/testing/index.md`
  - `scripts/qemu-test.sh`
- **first_action**: seed TASK-0020 execution plan in sequential order without reopening TASK-0019 scope.

## Start slice (now)
- **slice_name**: TASK-0020 kickoff prep (streams v2 planning + contract sync)
- **target_file**: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- **must_cover**:
  - keep TASK-0019 closed as done baseline,
  - preserve deterministic proof and marker contracts from completed slices,
  - keep TASK-0028 and TASK-0188 as ABI follow-on boundaries,
  - define bounded deterministic plan for TASK-0020 without cross-scope drift.

## Execution order
1. **TASK-0017**: remote statefs RW (Done)
2. **TASK-0018**: crashdumps v1 (Done, archived handoff)
3. **TASK-0019**: ABI syscall guardrails v2 (Done)
4. **TASK-0020+**: networking/state follow-ons (out of current slice)

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - SSOT and handoff now point to TASK-0019 done state.
- **best_system_solution**: YES
  - guardrail-first/policyd-only approach fits kernel-unchanged constraints.
- **scope_clear**: YES
  - scope remains v2 ABI guardrails only (not TASK-0028 learn/generator, not TASK-0188 kernel seccomp).
- **touched_paths_allowlist_present**: YES
  - TASK-0019 includes explicit allowlist for ABI/policy/selftest/docs/harness paths.

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - TASK-0028 and TASK-0188 are now explicit in TASK-0019 header.
- **security_considerations_complete**: YES
  - threat model, invariants, DON'T DO, and required negative tests include auth/spoofing/overflow rejects.

## Dependencies & blockers
- **blocked_by**: none (proof gates are green)
- **prereqs_ready**: YES
  - `TASK-0006`, `TASK-0008`, and `TASK-0009` are complete and referenced.
  - canonical QEMU harness contract and ABI markers are green in `scripts/qemu-test.sh` proof run.

## Decision
- **status**: READY FOR NEXT TASK (TASK-0019 done; TASK-0020 is next queue head)
- **notes**:
  - `policyd` remains explicit authority for TASK-0019 profile distribution.
  - marker/server wording is stable and proven by QEMU run.
  - `RFC-0032` contract remains complete; follow-on lifecycle scope remains TASK-0028.
  - keep kernel-seccomp claims out of TASK-0019 scope; continue in TASK-0188 only.
