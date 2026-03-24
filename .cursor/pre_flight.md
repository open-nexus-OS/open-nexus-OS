# Pre-Flight (End-of-Task Quality Gate)

<!--
CONTEXT
Run this checklist before updating task status to Done.
This is the "everything green" guard against fake success.
-->

## Automatic (must be green when applicable)
- [ ] Host diagnostics compile (when host code touched): `just diag-host`
- [ ] Narrow host/unit tests pass (when extracted seams get tests): use the task-specific canonical command (for `TASK-0016B`: `cargo test -p netstackd --tests -- --nocapture`)
- [ ] OS dependency gate (when OS code touched): `just dep-gate`
- [ ] OS diagnostics compile (when OS code touched): `just diag-os`
- [ ] Single-VM QEMU marker proof is green: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [ ] Cross-VM QEMU proof is green when the task/harness path requires it: `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- [ ] If `tools/os2vm.sh` path was used, summary artifacts were reviewed: `os2vm-summary-<runId>.json` and `.txt`
- [ ] For failed cross-VM runs, first-failure classification uses typed `OS2VM_E_*` matrix before new fixes
- [ ] QEMU runs were executed sequentially (no parallel smoke/harness runs contending on shared artifacts)
- [ ] Determinism floor respected: existing marker order and bounded retry semantics preserved
- [ ] No new lints in touched files (run lints per task or workspace policy)

## Manual (agent verifies, then documents proof)
- [ ] Acceptance Criteria satisfied (from task + linked RFC)
- [ ] Tests validate the **specified desired behavior** (Soll-Zustand), not current implementation quirks
- [ ] No fake-success logs/markers introduced (ready/ok only after real behavior)
- [ ] Marker semantics stay deterministic and evidence reflects real behavior for the active task
- [ ] Ownership/authority boundaries stayed aligned with the linked RFC/task contracts
- [ ] No follow-on feature scope leaked into this task

## Task-0016B manual addendum (when applicable)
- [ ] `netstackd` remains the networking owner per `TASK-0003` / `RFC-0006`
- [ ] `main.rs` is reduced toward entry/wiring only; orchestration lives behind explicit internal seams
- [ ] IPC wire format and reply semantics remain unchanged
- [ ] Loop/retry ownership remains explicit and bounded; no hidden unbounded helpers
- [ ] Daemon-path `unwrap`/`expect` are removed or narrowed away from runtime paths
- [ ] Newtypes / typed helpers improve clarity without changing public behavior
- [ ] No follow-on scope (`TASK-0194`, `TASK-0196`, `TASK-0249`) was silently absorbed

## Post-implementation (before claiming "Done")
- [ ] **Task docs**: active task file still matches reality
- [ ] Tests/proofs referenced in handoff and task evidence section
- [ ] OS build hygiene passed (if OS code touched): `just dep-gate`, `just diag-os`
- [ ] Host diagnostics passed when applicable: `just diag-host`
- [ ] No `unwrap`/`expect` on untrusted data in services
- [ ] Focused negative/unit tests exist if extracted seams justify them
- [ ] Markers honest (no `ready/ok` for stub paths)
- [ ] Headers updated (CONTEXT, TEST_COVERAGE, ADR)
- [ ] Docs synced only where touched by the task
- [ ] If `tools/os2vm.sh` was touched, typed summary output and rule matrix remain consistent with SSOT docs
- [ ] Follow-up boundaries are documented (no implicit scope creep into later tasks)
- [ ] If task changed marker expectations/gates, `.cursor/current_state.md`, `.cursor/handoff/current.md`, and `.cursor/next_task_prep.md` are updated in the same slice
- [ ] If the refactor reveals a missing external contract, stop and decide whether a new RFC/ADR is required before merging
