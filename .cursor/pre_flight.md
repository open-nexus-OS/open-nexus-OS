# Pre-Flight (End-of-Task Quality Gate)

<!--
CONTEXT
Run this checklist before updating task status to Done.
This is the "everything green" guard against fake success.
-->

## Automatic (must be green when applicable)
- [ ] Host diagnostics compile (when host code touched): `just diag-host`
- [ ] Narrow host/unit tests pass (when extracted seams get tests): `cargo test -p dsoftbusd -- --nocapture`
- [ ] OS dependency gate (when OS code touched): `just dep-gate`
- [ ] OS diagnostics compile (when OS code touched): `just diag-os`
- [ ] Single-VM QEMU marker proof is green: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [ ] Cross-VM QEMU proof is green when the refactor touches the cross-VM path: `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- [ ] If `tools/os2vm.sh` path was used, summary artifacts were reviewed: `os2vm-summary-<runId>.json` and `.txt`
- [ ] For failed cross-VM runs, first-failure classification uses typed `OS2VM_E_*` matrix before new fixes
- [ ] QEMU runs were executed sequentially (no parallel smoke/harness runs contending on shared artifacts)
- [ ] Determinism floor respected: existing marker order and bounded retry semantics preserved
- [ ] No new lints in touched files (run lints per task or workspace policy)

## Manual (agent verifies, then documents proof)
- [ ] Acceptance Criteria satisfied (from task + linked RFC)
- [ ] Tests validate the **specified desired behavior** (Soll-Zustand), not current implementation quirks
- [ ] No fake-success logs/markers introduced (ready/ok only after real behavior)
- [ ] Remote packagefs surface is read-only only (`stat/open/read/close`)
- [ ] Path canonicalization keeps resolution inside packagefs namespace (`pkg:/` or `/packages/`)
- [ ] Security rejects are fail-closed for unauthenticated, traversal, non-scheme, and oversize requests
- [ ] Marker semantics stay deterministic and evidence reflects real remote packagefs behavior (marker + packet evidence where available)
- [ ] No follow-on feature scope (`TASK-0017`, `TASK-0020`, `TASK-0021`, `TASK-0022`) leaked into this task

## Post-implementation (before claiming "Done")

- [ ] **Task docs**: `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md` still matches reality
- [ ] Tests/proofs referenced in handoff and task evidence section
- [ ] OS build hygiene passed (if OS code touched): `just dep-gate`, `just diag-os`
- [ ] Host diagnostics passed when applicable: `just diag-host`
- [ ] No `unwrap`/`expect` on untrusted data in services
- [ ] Focused negative/unit tests exist if extracted seams justify them
- [ ] Markers honest (no `ready/ok` for stub paths)
- [ ] Headers updated (CONTEXT, TEST_COVERAGE, ADR)
- [ ] Docs synced only where touched by the task (`docs/distributed/dsoftbus-lite.md`, `docs/testing/index.md`, `docs/testing/network-distributed-debugging.md`)
- [ ] If `tools/os2vm.sh` was touched, typed summary output and rule matrix remain consistent with SSOT docs
- [ ] Follow-up boundaries are documented (no implicit scope creep into later tasks)
- [ ] If task changed marker expectations/gates, `.cursor/current_state.md`, `.cursor/handoff/current.md`, and `.cursor/next_task_prep.md` are updated in the same slice
- [ ] If the refactor reveals a missing external contract, stop and decide whether a new RFC/ADR is required before merging
