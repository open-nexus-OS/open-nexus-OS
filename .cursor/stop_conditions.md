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

## TASK-0019 class stop conditions (ABI syscall guardrails v2)
- [ ] Kernel remains untouched; task documents this as a userland guardrail (not kernel sandboxing).
- [ ] Compliant syscall path is centralized through `nexus-abi` wrappers with deterministic deny behavior.
- [ ] Profile parsing and rule matching are bounded + deterministic (no unbounded rule/path/arg parsing).
- [ ] Profile distribution enforces authenticated authority (`sender_service_id`) and subject-binding rejects.
- [ ] Phased rollout evidence is explicit and complete (critical-service phases + final all-shipped-components coverage claim).
- [ ] Deny-by-default behavior is explicit and proven (`default deny` when no rule matches).
- [ ] Deny decisions are auditable with deterministic labels/fields.
- [ ] Lifecycle boundary is enforced for TASK-0019:
  - [ ] boot/startup apply only in this task,
  - [ ] no runtime mode switch/hot reload path is introduced here,
  - [ ] runtime lifecycle scope is explicitly deferred to `TASK-0028`.
- [ ] Required negative tests are green:
  - [ ] `test_reject_unbounded_profile`
  - [ ] `test_reject_unauthenticated_profile_distribution`
  - [ ] `test_reject_subject_spoofed_profile_identity`
  - [ ] `test_reject_profile_rule_count_overflow`
- [ ] Host proof is green:
  - [ ] profile parsing/matching precedence tests
  - [ ] stable error mapping tests
  - [ ] authenticated profile ingestion/subject binding tests
- [ ] OS proof is green and sequential:
  - [ ] `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - [ ] `tools/os2vm.sh` only if explicitly required by task scope
- [ ] QEMU marker ladder includes and proves:
  - [ ] `SELFTEST: abi filter deny ok`
  - [ ] `SELFTEST: abi filter allow ok`
  - [ ] `SELFTEST: abi netbind deny ok`
- [ ] Build hygiene stays green when OS code is touched:
  - [ ] `just dep-gate`
  - [ ] `just diag-os`
- [ ] No unresolved RED decision points remain in `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`.
- [ ] No follow-on scope (`TASK-0028`, `TASK-0188`) was silently absorbed.

## Legacy stop-condition profiles (reference only)
- [ ] TASK-0018-class crashdump v1 stop conditions: use archived closeout evidence in `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md` (`Done`).
- [ ] TASK-0017-class remote statefs RW ACL/audit stop conditions: use archived closeout evidence in `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md` (`Done`).

## Never claim success if…
- [ ] Tests were not run where applicable.
- [ ] Markers say `ok/ready` but behavior is stubbed.
- [ ] Scope expanded beyond touched-path allowlist without explicit plan/task update.
- [ ] QEMU proofs were run in parallel and produced contention artifacts.
- [ ] Wire layout, retry budgets, ACL/audit behavior, or marker semantics changed silently.
