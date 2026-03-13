# Stop Conditions (Task Completion)

<!--
CONTEXT
Hard stop conditions: a task is not "done" unless these are satisfied.
This prevents subjective completion and reduces drift across sessions.
-->

## Task completion stop conditions (must satisfy all applicable)
- [ ] All MUST Acceptance Criteria are implemented and verified with proof
- [ ] All stated Security Invariants are enforced and have negative tests where appropriate (`test_reject_*`)
- [ ] No regressions against `.cursor/current_state.md` constraints/invariants
- [ ] Proof artifacts exist and are referenced in the task/handoff (tests, QEMU markers, logs)
- [ ] For TASK-0015-class daemon refactor work:
  - [ ] `main.rs` is reduced to thin entry/wiring responsibilities
  - [ ] Internal seams exist for transport IPC, discovery, session lifecycle, gateway/local IPC, and observability
  - [ ] Existing DSoftBus wire formats are unchanged
  - [ ] Existing marker names and marker semantics are unchanged
  - [ ] Single-VM proof remains green
  - [ ] Cross-VM proof remains green when the cross-VM path was touched
  - [ ] Any `tools/os2vm.sh` edits are harness-only parity updates (no silent marker/wire contract drift)
  - [ ] No unresolved RED decision points remain in the task file
  - [ ] No speculative feature modules were introduced just to mirror future tasks
- [ ] Header blocks updated to reflect:
  - [ ] API stability impact (if any)
  - [ ] Test coverage (what exists, where, and how to run)
  - [ ] ADR/RFC references (if boundaries/contracts were touched)
- [ ] Documentation updated when it is a contract surface:
  - [ ] docs/arch (if architecture changed)
  - [ ] docs/testing (if new tests/markers introduced)
  - [ ] README/guide docs (if developer workflow changed)

## Never claim success if…
- [ ] Tests were not run when they exist and are applicable
- [ ] Markers say `ok/ready` but behavior is stubbed (must say `stub/placeholder`)
- [ ] Scope expanded beyond touched-path allowlist without explicit note and plan update
- [ ] QEMU proofs were run in parallel and produced lock/contention artifacts instead of deterministic evidence
- [ ] Ownership/newtype/Send-Sync boundary changes were made without task/RFC/header synchronization
- [ ] The refactor silently changed wire layout, retry budgets, or remote-proxy behavior while still claiming “no behavior change”
