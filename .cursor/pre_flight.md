# Pre-Flight (End-of-Task Quality Gate)

<!--
CONTEXT
Run this checklist before updating a task status to Done.
This is the anti-fake-success gate.
-->

## Automatic (must be green when applicable)
- [x] Host diagnostics compile (when host code touched): `just diag-host`
- [x] Narrow host/unit tests pass (task canonical command from active task doc; for TASK-0018: host minidump/symbolization/reject-path tests)
- [x] OS dependency gate (when OS code touched): `just dep-gate`
- [x] OS diagnostics compile (when OS code touched): `just diag-os`
- [x] Single-VM QEMU marker proof is green: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [ ] Cross-VM QEMU proof is green when the active task explicitly requires it: `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- [ ] If `tools/os2vm.sh` path was used, summary artifacts were reviewed (`os2vm-summary-<runId>.json` and `.txt`)
- [x] QEMU runs were executed sequentially (no parallel smoke/harness runs contending on shared artifacts)
- [x] Determinism floor respected: marker order and bounded retry semantics preserved
- [x] No new lints in touched files (run task/workspace lint policy)

## Manual (agent verifies, then documents proof)
- [x] Acceptance Criteria satisfied (task + linked RFC/ADR)
- [x] Tests validate the desired behavior (Soll-Zustand), not implementation quirks
- [x] No fake-success logs/markers introduced (`ready/ok` only after real behavior)
- [x] Ownership/authority boundaries stayed aligned with linked contracts
- [x] No follow-on feature scope leaked into this task

## Task-0018 manual addendum (when applicable)
- [x] Capture path is in-process only (no ptrace-like post-mortem assumptions).
- [x] Dump paths are normalized and constrained to `/state/crash/...`.
- [x] Dump payload caps are enforced (stack/code/total frame are bounded and tested).
- [x] Malformed/oversized/path-escape inputs are rejected deterministically (`test_reject_*`).
- [x] Crash event includes deterministic `build_id` + `dump_path` semantics where available.
- [x] Markers `execd: minidump written` and `SELFTEST: minidump ok` are emitted only on real success.
- [x] Host symbolization proof is green and kept host-first for v1 (no fake OS symbolization claims).
- [x] No follow-on scope (`TASK-0048`, `TASK-0049`, `TASK-0141`, `TASK-0142`, `TASK-0227`) was silently absorbed.
- [x] Phase 3 drift lock documented in TASK/RFC and aligned with follow-up task contracts.

## Legacy manual profiles (reference only)
- [ ] TASK-0017 remote statefs ACL/audit checks are tracked in archived task closeout and its task-local evidence.

## Post-implementation (before claiming "Done")
- [x] Task doc still matches reality (status, proofs, touched paths)
- [x] Proof commands and evidence are mirrored in handoff/task sections
- [x] Header blocks updated (CONTEXT, TEST_COVERAGE, ADR links) where code was touched
- [x] Docs synced only where contract/proof surfaces changed
- [x] `.cursor/current_state.md`, `.cursor/handoff/current.md`, `.cursor/next_task_prep.md`, `.cursor/stop_conditions.md` updated in same slice
