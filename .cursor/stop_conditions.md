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

## TASK-0018 class stop conditions (crashdumps v1)
- [ ] v1 capture path is in-process only and kernel remains untouched.
- [ ] Dump artifact path is normalized and constrained to `/state/crash/...`.
- [ ] Dump framing is deterministic and bounded (no unbounded stack/code/full-memory capture).
- [ ] Malformed/oversized/path-escape crashdump inputs are rejected fail-closed with deterministic behavior.
- [ ] Crash event path is emitted with deterministic metadata (`build_id`, `dump_path`) where available.
- [ ] Marker proofs are honest-green:
  - [ ] `execd: minidump written`
  - [ ] `SELFTEST: minidump ok`
- [ ] Host-first proof is green:
  - [ ] task-defined host minidump/symbolization proof command is green
  - [ ] required negative tests are present and passing (`test_reject_*` for malformed/oversized/path/auth where applicable)
- [ ] OS proof is green and sequential:
  - [ ] `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - [ ] `tools/os2vm.sh` only if explicitly required by task scope
- [ ] Build hygiene stays green when OS code is touched:
  - [ ] `just dep-gate`
  - [ ] `just diag-os`
- [ ] No unresolved RED decision points remain in `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`.
- [ ] No follow-on scope (`TASK-0048`, `TASK-0049`, `TASK-0141`, `TASK-0142`, `TASK-0227`) was silently absorbed.

## Legacy stop-condition profiles (reference only)
- [ ] TASK-0017-class remote statefs RW ACL/audit stop conditions: use archived closeout evidence in `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`.
- [ ] TASK-0016-class remote packagefs RO stop conditions: use task-local checklist in `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`.
- [ ] TASK-0016B-class netstackd modularization stop conditions: use task-local checklist in `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md`.

## Never claim success if…
- [ ] Tests were not run where applicable.
- [ ] Markers say `ok/ready` but behavior is stubbed.
- [ ] Scope expanded beyond touched-path allowlist without explicit plan/task update.
- [ ] QEMU proofs were run in parallel and produced contention artifacts.
- [ ] Wire layout, retry budgets, ACL/audit behavior, or marker semantics changed silently.
