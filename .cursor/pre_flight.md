# Pre-Flight Quality Gate

<!--
CONTEXT
Quality gate checklist run at the end of each implementation task.

Design:
- Split into "automatic" vs "manual" checks.
- Prefer concrete commands and proof artifacts.
-->

## Automatic (must be green when applicable)
- [ ] Host diagnostics compile (when host code touched): `just diag-host`
- [ ] Host tests pass (when host tests exist/changed): `cargo test --workspace`
- [ ] OS dependency gate (when OS code touched): `just dep-gate`
- [ ] OS diagnostics compile (when OS code touched): `just diag-os`
- [ ] QEMU smoke tests / marker run (when behavior affects OS runtime): `just test-os` or `RUN_UNTIL_MARKER=1 just test-os`
- [ ] No new lints in touched files (run lints per task or workspace policy)

## Manual (agent verifies, then documents proof)
- [ ] Acceptance Criteria satisfied (from task + linked RFC)
- [ ] Tests validate the **specified desired behavior** (Soll-Zustand), not current implementation quirks
- [ ] No fake-success logs/markers introduced (ready/ok only after real behavior)
- [ ] Security invariants met (secrets, identity, bounded input, policy, W^X)
- [ ] Touched paths stayed within allowlist (or explicitly justified + recorded)
- [ ] Header blocks updated (CONTEXT/STATUS/TEST_COVERAGE/ADR references)
- [ ] Architecture docs/ADRs updated if contracts/boundaries changed
- [ ] `.cursor/current_state.md` updated (compressed "why" + open threads)
- [ ] `.cursor/handoff/current.md` updated (what's done/next/constraints)
