# Pre-Flight (End-of-Task Quality Gate)

<!--
CONTEXT
Run this checklist before updating a task status to Done.
This is the anti-fake-success gate.
-->

## Automatic (must be green when applicable)
- [x] Host diagnostics compile (when host code touched): `just diag-host`
- [x] Narrow host/unit tests pass (task canonical command from active task doc; for TASK-0019: ABI matcher/auth/audit reject tests)
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
- [x] Header discipline checked in touched code/docs (CONTEXT/OWNERS/TEST_COVERAGE where applicable)
- [x] Rust construct hygiene reviewed where relevant (`newtype` candidates, ownership boundaries, `#[must_use]` for critical return values)

## Task-0019 manual addendum (when applicable)
- [x] Scope stays kernel-unchanged and explicitly non-sandbox for malicious raw `ecall`.
- [x] `nexus-abi` wrapper path remains the single compliant syscall entry point.
- [x] Profile parsing/matching is deterministic and bounded (rule count/path/arg sizes).
- [x] Profile distribution is authenticated (`sender_service_id`) with subject-binding rejects.
- [x] Rollout evidence is phased and explicit (selected services first, then full shipped OS component coverage).
- [x] Deny decisions emit deterministic audit evidence (logd-backed or deterministic fallback path).
- [x] Negative tests exist for unbounded profile, unauthenticated distribution, subject spoofing, and rule overflow.
- [x] TASK-0019 lifecycle boundary holds: boot/startup apply only, no runtime mode switch/hot reload in this task.
- [x] QEMU markers include `abi-profile: ready (server=policyd|abi-filterd)` and `abi-filter: deny (subject=<svc> syscall=<op>)`.
- [x] QEMU markers include `SELFTEST: abi filter deny ok`, `SELFTEST: abi filter allow ok`, and `SELFTEST: abi netbind deny ok`.
- [x] Follow-on boundaries (`TASK-0028`, `TASK-0188`) are documented and not absorbed.

## Legacy manual profiles (reference only)
- [ ] TASK-0018 crashdump closeout checks are tracked in archived handoff and task-local evidence (`Done`).

## Post-implementation (before claiming "Done")
- [x] Task doc still matches reality (status, proofs, touched paths)
- [x] Proof commands and evidence are mirrored in handoff/task sections
- [x] Header blocks updated (CONTEXT, TEST_COVERAGE, ADR links) where code was touched
- [x] Docs synced only where contract/proof surfaces changed
- [x] `.cursor/current_state.md`, `.cursor/handoff/current.md`, `.cursor/next_task_prep.md`, `.cursor/stop_conditions.md` updated in same slice
