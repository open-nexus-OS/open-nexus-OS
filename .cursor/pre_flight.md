# Pre-Flight (End-of-Task Quality Gate)

<!--
CONTEXT
Run this checklist before updating task status to Done.
This is the "everything green" guard against fake success.
-->

## Automatic (must be green when applicable)
- [ ] Host diagnostics compile (when host code touched): `just diag-host`
- [ ] Host tests pass (when host tests exist/changed): `cargo test --workspace`
- [ ] OS dependency gate (when OS code touched): `just dep-gate`
- [ ] OS diagnostics compile (when OS code touched): `just diag-os`
- [ ] QEMU smoke tests / marker run (when behavior affects OS runtime): `just test-os` or `RUN_UNTIL_MARKER=1 just test-os`
- [ ] SMP tasks: dual-mode QEMU proof is green:
  - [ ] `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` (SMP marker gate enabled)
  - [ ] `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` (default smoke semantics preserved)
- [ ] QEMU runs were executed sequentially (no parallel smoke/harness runs contending on shared artifacts)
- [ ] Determinism floor respected: modern virtio-mmio default used for green proofs (legacy mode only for debug/bisect)
- [ ] No new lints in touched files (run lints per task or workspace policy)

## Manual (agent verifies, then documents proof)
- [ ] Acceptance Criteria satisfied (from task + linked RFC)
- [ ] Tests validate the **specified desired behavior** (Soll-Zustand), not current implementation quirks
- [ ] No fake-success logs/markers introduced (ready/ok only after real behavior)

## Post-implementation (before claiming "Done")

- [ ] **RFC**: Status updated to `Complete` if all proofs green; Implementation Checklist filled
- [ ] **RFC README**: Entry updated with correct status in `docs/rfcs/README.md`
- [ ] Tests green (`just test-host`, `just test-e2e`, and relevant QEMU markers)
- [ ] OS build hygiene passed (if OS code touched): `just dep-gate`, `just diag-os`
- [ ] Lint + format passed: `just lint`, `just fmt-check`
- [ ] No `unwrap`/`expect` on untrusted data in services
- [ ] Security negative tests (`test_reject_*`) exist if security-relevant
- [ ] Markers honest (no `ready/ok` for stub paths)
- [ ] Headers updated (CONTEXT, TEST_COVERAGE, ADR)
- [ ] Docs synced (architecture, testing, contracts)
- [ ] If task changed marker expectations/gates, `.cursor/current_state.md`, `.cursor/handoff/current.md`, and `.cursor/next_task_prep.md` are updated in the same slice
