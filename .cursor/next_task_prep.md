# Next Task Preparation (Drift-Free)

<!--
CONTEXT
Preparation file for the next execution slice.
Update during wrap-up so a new session can start without drift.
-->

## Candidate next task
- **task**: `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`
- **handoff_target**: `.cursor/handoff/current.md`
- **handoff_archive**: `.cursor/handoff/archive/TASK-0017-dsoftbus-remote-statefs-rw.md`
- **linked_contracts**:
  - `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`
  - `docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md`
  - `docs/rfcs/RFC-0011-logd-journal-crash-v1.md`
  - `docs/rfcs/RFC-0018-statefs-journal-format-v1.md`
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
  - `docs/testing/index.md`
  - `scripts/qemu-test.sh`
- **first_action**: run final TASK-0018 stop-condition gap check, prepare commit proposal, then open identity-hardening follow-up (no scope leak into crash v2 tasks).

## Start slice (now)
- **slice_name**: TASK-0018 Phase 4 closeout (identity/report hardening + explicit negative E2E rejects)
- **target_file**: `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`
- **must_cover**:
  - deterministic and bounded in-process crashdump capture
  - `/state/crash/...` artifact path normalization and bounded writes
  - deterministic crash event publication (`build_id`, `dump_path`)
  - host-first symbolization proof for known build-id/PC fixtures
  - fail-closed reject behavior for malformed/oversized/path-escape inputs
  - no unresolved RED decision points in v1 scope

## Execution order
1. **TASK-0017**: remote statefs RW ACL/audit (Archived closeout)
2. **TASK-0018**: crashdumps v1 (current active slice)
3. **TASK-0048 / TASK-0049 / TASK-0141 / TASK-0227**: crash follow-ons (out of scope for this slice)

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - current SSOT now points at TASK-0018 kickoff.
- **best_system_solution**: YES
  - keeps v1 minimal, deterministic, and kernel-unchanged.
- **scope_clear**: YES
  - task is crashdump v1 only, not v2 crash pipeline/export/bundle work.
- **touched_paths_allowlist_present**: YES
  - task allowlist constrained to crash/execd/selftest/tools/harness/docs paths.

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - TASK-0048, TASK-0049, TASK-0141, TASK-0142, TASK-0227 are explicitly linked.
- **security_considerations_complete**: YES
  - threat model, invariants, DON'T DO, and required negative tests are explicit.

## Dependencies & blockers
- **blocked_by**: none (proof baseline already green)
- **prereqs_ready**: YES
  - `TASK-0006` and `TASK-0009` are completed and available.
  - canonical QEMU harness and observability/state contracts exist.

## Decision
- **status**: READY (TASK-0018 final closeout check + commit proposal)
- **notes**:
  - keep v1 symbolization proof host-first; do not claim OS DWARF symbolization.
  - keep follow-on scope (`TASK-0048`/`TASK-0049`/`TASK-0141`/`TASK-0142`/`TASK-0227`) out of the commit.
  - identity-hardening cleanup (remove proof-path sender canonicalization) is a separate follow-up slice.
