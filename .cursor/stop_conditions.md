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
- [ ] For `tools/os2vm.sh`-based proof, typed summary artifacts are present and linked (`os2vm-summary-*.json` / `.txt`)
- [ ] For TASK-0016-class remote packagefs RO work:
  - [ ] Exposed remote operations are read-only only (`stat/open/read/close`)
  - [ ] Non-packagefs paths/schemes are rejected fail-closed
  - [ ] Path traversal attempts are rejected fail-closed
  - [ ] Request bounds (path/read/chunk/handle limits) are enforced with deterministic errors
  - [ ] Existing DSoftBus wire/marker semantics remain stable unless explicitly revised in task evidence
  - [ ] Single-VM proof remains green
  - [ ] Cross-VM proof remains green when cross-VM path was touched
  - [ ] Any `tools/os2vm.sh` edits are harness-only parity updates (no silent marker/wire contract drift)
  - [ ] No unresolved RED decision points remain in the task file
  - [ ] No follow-on scope (`TASK-0017`, `TASK-0020`, `TASK-0021`, `TASK-0022`) was silently absorbed
- [ ] For TASK-0016B-class `netstackd` structural refactor work:
  - [ ] `source/services/netstackd/src/main.rs` is reduced to entry/wiring only
  - [ ] Bootstrap, IPC/wire handling, handle ownership, loopback shim, and facade ops live behind explicit internal seams
  - [ ] Existing `netstackd` wire and marker semantics remain stable unless explicitly revised in task evidence
  - [ ] `netstackd` remains the networking owner per `TASK-0003` / `RFC-0006`
  - [ ] No duplicate authority or MMIO bypass path was introduced
  - [ ] Loop/retry ownership remains explicit and bounded
  - [ ] Daemon-path `unwrap`/`expect` are removed from runtime-sensitive paths
  - [ ] Narrow host tests exist for extracted seams where practical
  - [ ] Single-VM proof remains green
  - [ ] Cross-VM regression remains green when the harness path was touched or relied on
  - [ ] Any `scripts/qemu-test.sh` / `tools/os2vm.sh` edits are parity updates only (no silent contract drift)
  - [ ] No unresolved RED decision points remain in the task file
  - [ ] No follow-on scope (`TASK-0194`, `TASK-0196`, `TASK-0249`) was silently absorbed
- [ ] Header blocks updated to reflect:
  - [ ] API stability impact (if any)
  - [ ] Test coverage (what exists, where, and how to run)
  - [ ] ADR/RFC references (if boundaries/contracts were touched)
- [ ] Documentation updated when it is a contract surface:
  - [ ] docs/arch (if architecture changed)
  - [ ] docs/testing (if new tests/markers introduced), including SSOT sync in `docs/testing/network-distributed-debugging.md`
  - [ ] README/guide docs (if developer workflow changed)

## Never claim success if…
- [ ] Tests were not run when they exist and are applicable
- [ ] Markers say `ok/ready` but behavior is stubbed (must say `stub/placeholder`)
- [ ] Scope expanded beyond touched-path allowlist without explicit note and plan update
- [ ] QEMU proofs were run in parallel and produced lock/contention artifacts instead of deterministic evidence
- [ ] Ownership/newtype/Send-Sync boundary changes were made without task/RFC/header synchronization
- [ ] The refactor silently changed wire layout, retry budgets, or remote-proxy behavior while still claiming “no behavior change”
- [ ] `os2vm` typed rule-matrix and docs SSOT drifted (script behavior no longer matches `docs/testing/network-distributed-debugging.md`)
- [ ] `netstackd` structural cleanup quietly turned explicit fatal/terminal bring-up policy into hidden fallback success
