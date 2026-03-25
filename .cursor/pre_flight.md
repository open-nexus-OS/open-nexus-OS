# Pre-Flight (End-of-Task Quality Gate)

<!--
CONTEXT
Run this checklist before updating a task status to Done.
This is the anti-fake-success gate.
-->

## Automatic (must be green when applicable)
- [ ] Host diagnostics compile (when host code touched): `just diag-host`
- [ ] Narrow host/unit tests pass (task canonical command; for TASK-0017: `cargo test -p dsoftbusd --tests -- --nocapture`)
- [ ] OS dependency gate (when OS code touched): `just dep-gate`
- [ ] OS diagnostics compile (when OS code touched): `just diag-os`
- [ ] Single-VM QEMU marker proof is green: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [ ] Cross-VM QEMU proof is green when task path requires it: `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- [ ] If `tools/os2vm.sh` path was used, summary artifacts were reviewed (`os2vm-summary-<runId>.json` and `.txt`)
- [ ] QEMU runs were executed sequentially (no parallel smoke/harness runs contending on shared artifacts)
- [ ] Determinism floor respected: marker order and bounded retry semantics preserved
- [ ] No new lints in touched files (run task/workspace lint policy)

## Manual (agent verifies, then documents proof)
- [ ] Acceptance Criteria satisfied (task + linked RFC/ADR)
- [ ] Tests validate the desired behavior (Soll-Zustand), not implementation quirks
- [ ] No fake-success logs/markers introduced (`ready/ok` only after real behavior)
- [ ] Ownership/authority boundaries stayed aligned with linked contracts
- [ ] No follow-on feature scope leaked into this task

## Task-0017 manual addendum (when applicable)
- [ ] Remote RW ACL is deny-by-default (`/state/shared/*` only)
- [ ] Prefix escape/path normalization bypasses are rejected fail-closed
- [ ] Oversize key/value requests are rejected with deterministic errors
- [ ] Unauthenticated/identity-spoofed remote requests are rejected fail-closed
- [ ] Every remote `PUT`/`DELETE` emits deterministic audit evidence
- [ ] Markers `dsoftbusd: remote statefs served` and `SELFTEST: remote statefs rw ok` are emitted only on real success
- [ ] No follow-on scope (`TASK-0020`, `TASK-0021`, `TASK-0022`) was silently absorbed

## Post-implementation (before claiming "Done")
- [ ] Task doc still matches reality (status, proofs, touched paths)
- [ ] Proof commands and evidence are mirrored in handoff/task sections
- [ ] Header blocks updated (CONTEXT, TEST_COVERAGE, ADR links) where code was touched
- [ ] Docs synced only where contract/proof surfaces changed
- [ ] `.cursor/current_state.md`, `.cursor/handoff/current.md`, `.cursor/next_task_prep.md`, `.cursor/stop_conditions.md` updated in same slice
