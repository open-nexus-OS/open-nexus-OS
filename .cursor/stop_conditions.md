# Stop Conditions (Task Completion)

<!--
CONTEXT
Hard stop conditions: a task is not "Done" unless all applicable items are satisfied.
-->

## Task completion stop conditions (must satisfy all applicable)
- [ ] All MUST Acceptance Criteria are implemented and proven.
- [ ] All stated Security Invariants are enforced and have negative tests where applicable (`test_reject_*`).
- [ ] No regressions against `.cursor/current_state.md` constraints/invariants.
- [ ] Proof artifacts exist and are referenced in task/handoff docs (tests, markers, logs).
- [ ] For `tools/os2vm.sh` proof paths, typed summary artifacts are present and linked (`os2vm-summary-*.json` / `.txt`).
- [ ] Header blocks and docs are updated where boundaries/contracts/proofs changed.

## TASK-0017 class stop conditions (remote statefs RW ACL/audit)
- [ ] Remote RW is authenticated and deny-by-default.
- [ ] Only `/state/shared/*` (or explicitly declared equivalent) is writable remotely.
- [ ] Prefix-escape/path-normalization bypass attempts are rejected fail-closed.
- [ ] Oversize keys/values/requests are rejected with deterministic bounded errors.
- [ ] Unauthorized/unauthenticated requests are rejected fail-closed.
- [ ] Every remote `PUT`/`DELETE` emits deterministic audit evidence (logd or deterministic fallback marker).
- [ ] Marker proofs are honest-green:
  - [ ] `dsoftbusd: remote statefs served`
  - [ ] `SELFTEST: remote statefs rw ok`
- [ ] Host-first proof is green:
  - [ ] `cargo test -p dsoftbusd --tests -- --nocapture`
  - [ ] required negative tests are present and passing:
    - [ ] `test_reject_statefs_write_outside_acl`
    - [ ] `test_reject_statefs_prefix_escape`
    - [ ] `test_reject_oversize_statefs_write`
    - [ ] `test_reject_unauthenticated_statefs_request`
- [ ] OS proof is green and sequential:
  - [ ] `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - [ ] `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- [ ] Build hygiene stays green:
  - [ ] `just dep-gate`
  - [ ] `just diag-os`
- [ ] No unresolved RED decision points remain in `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`.
- [ ] No follow-on scope (`TASK-0020`, `TASK-0021`, `TASK-0022`) was silently absorbed.

## Legacy stop-condition profiles (reference only)
- [ ] TASK-0016-class remote packagefs RO stop conditions: use task-local checklist in `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`.
- [ ] TASK-0016B-class netstackd modularization stop conditions: use task-local checklist in `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md`.

## Never claim success if…
- [ ] Tests were not run where applicable.
- [ ] Markers say `ok/ready` but behavior is stubbed.
- [ ] Scope expanded beyond touched-path allowlist without explicit plan/task update.
- [ ] QEMU proofs were run in parallel and produced contention artifacts.
- [ ] Wire layout, retry budgets, ACL/audit behavior, or marker semantics changed silently.
