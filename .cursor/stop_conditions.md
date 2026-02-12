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
- [ ] For QoS/timed tasks:
  - [ ] QoS set authorization semantics are explicit and tested (`self` vs privileged `other-pid`)
  - [ ] Invalid QoS class and unauthorized set paths have reject tests
  - [ ] Syscall argument decoding does not silently truncate/clamp wire values before validation (overflow rejects deterministically)
  - [ ] Timer registration/coalescing bounds are explicit and have reject tests
  - [ ] SMP marker semantics remain unchanged (no drift from TASK-0012/TASK-0012B)
- [ ] For SMP/parallelism tasks:
  - [ ] SMP>=2 proof is green (explicit marker-gated run)
  - [ ] SMP=1 regression proof is green (default smoke behavior unchanged)
  - [ ] No unresolved RED decision points remain in the task file
  - [ ] Queue capacity/backpressure semantics are explicit (reject/defer behavior documented and tested)
  - [ ] CPU-ID fast-path/fallback contract is explicit and proven deterministic
  - [ ] Trap/IPI hardening preserves existing marker meaning (no semantic drift)
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
