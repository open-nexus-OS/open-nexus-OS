# Stop Conditions (Task Completion)

<!--
CONTEXT
Hard stop conditions: a task is not "Done" unless all applicable items are satisfied.
-->

## Task completion stop conditions (must satisfy all applicable)
- [x] All MUST Acceptance Criteria are implemented and proven.
- [x] All stated Security Invariants are enforced and have negative tests where applicable (`test_reject_*`).
- [x] No regressions against `.cursor/current_state.md` constraints/invariants.
- [x] Proof artifacts exist and are referenced in task/handoff docs (tests, markers, logs).
- [ ] For `tools/os2vm.sh` proof paths, typed summary artifacts are present and linked (`os2vm-summary-*.json` / `.txt`).
- [x] Header blocks and docs are updated where boundaries/contracts/proofs changed.
- [x] Rust API hygiene is reviewed for touched paths (`newtype`/ownership/`#[must_use]` where sensible).

## TASK-0019 class stop conditions (ABI syscall guardrails v2)
- [x] Kernel remains untouched; task documents this as a userland guardrail (not kernel sandboxing).
- [x] Compliant syscall path is centralized through `nexus-abi` wrappers with deterministic deny behavior.
- [x] Profile parsing and rule matching are bounded + deterministic (no unbounded rule/path/arg parsing).
- [x] Profile distribution enforces authenticated authority (`sender_service_id`) and subject-binding rejects.
- [x] Phased rollout evidence is explicit and complete (critical-service phases + final all-shipped-components coverage claim).
- [x] Deny-by-default behavior is explicit and proven (`default deny` when no rule matches).
- [x] Deny decisions are auditable with deterministic labels/fields.
- [ ] Lifecycle boundary is enforced for TASK-0019:
  - [x] boot/startup apply only in this task,
  - [x] no runtime mode switch/hot reload path is introduced here,
  - [x] runtime lifecycle scope is explicitly deferred to `TASK-0028`.
- [ ] Required negative tests are green:
  - [x] `test_reject_unbounded_profile`
  - [x] `test_reject_unauthenticated_profile_distribution`
  - [x] `test_reject_subject_spoofed_profile_identity`
  - [x] `test_reject_profile_rule_count_overflow`
- [ ] Host proof is green:
  - [x] profile parsing/matching precedence tests
  - [x] stable error mapping tests
  - [x] authenticated profile ingestion/subject binding tests
- [ ] OS proof is green and sequential:
  - [x] `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - [ ] `tools/os2vm.sh` only if explicitly required by task scope
- [ ] QEMU marker ladder includes and proves:
  - [x] `abi-profile: ready (server=policyd|abi-filterd)`
  - [x] `abi-filter: deny (subject=<svc> syscall=<op>)`
  - [x] `SELFTEST: abi filter deny ok`
  - [x] `SELFTEST: abi filter allow ok`
  - [x] `SELFTEST: abi netbind deny ok`
- [ ] Build hygiene stays green when OS code is touched:
  - [x] `just dep-gate`
  - [x] `just diag-os`
- [x] No unresolved RED decision points remain in `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`.
- [x] No follow-on scope (`TASK-0028`, `TASK-0188`) was silently absorbed.

## Legacy stop-condition profiles (reference only)
- [ ] TASK-0018-class crashdump v1 stop conditions: use archived closeout evidence in `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md` (`Done`).
- [ ] TASK-0017-class remote statefs RW ACL/audit stop conditions: use archived closeout evidence in `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md` (`Done`).

## Never claim success if…
- [ ] Tests were not run where applicable.
- [ ] Markers say `ok/ready` but behavior is stubbed.
- [ ] Scope expanded beyond touched-path allowlist without explicit plan/task update.
- [ ] QEMU proofs were run in parallel and produced contention artifacts.
- [ ] Wire layout, retry budgets, ACL/audit behavior, or marker semantics changed silently.
