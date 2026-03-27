# Next Task Preparation (Drift-Free)

<!--
CONTEXT
Preparation file for the next execution slice.
Update during wrap-up so a new session can start without drift.
-->

## Candidate next task
- **task**: `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`
- **handoff_target**: `.cursor/handoff/current.md`
- **handoff_archive**: `.cursor/handoff/archive/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`
- **linked_contracts**:
  - `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`
  - `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md`
  - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
  - `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
  - `tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md`
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
  - `tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md` (follow-on boundary)
  - `tasks/TASK-0188-kernel-sysfilter-v1-task-profiles-rate-buckets.md` (kernel follow-on boundary)
  - `docs/testing/index.md`
  - `scripts/qemu-test.sh`
- **first_action**: execute TASK-0019 Phase A (bounded filter chain + deterministic deny/audit), then follow explicit phased rollout through distribution and marker closure.

## Start slice (now)
- **slice_name**: TASK-0019 phased kickoff (A-F rollout + lifecycle boundary lock)
- **target_file**: `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`
- **must_cover**:
  - kernel-unchanged userland ABI syscall guardrail architecture
  - deterministic deny-by-default profile parsing + bounded matching
  - authenticated profile distribution (`sender_service_id`) + subject binding
  - stable deny errors + audit evidence path
  - selftest marker ladder (`abi filter deny/allow`, `abi netbind deny`)
  - explicit non-goal boundary (no malicious-code sandbox claim)

## Execution order
1. **TASK-0017**: remote statefs RW (Done)
2. **TASK-0018**: crashdumps v1 (Done, archived handoff)
3. **TASK-0019**: ABI syscall guardrails v2 (next active prep/implementation slice)
4. **TASK-0020+**: networking/state follow-ons (out of current slice)

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - SSOT and handoff now point to TASK-0019 prep.
- **best_system_solution**: YES
  - guardrail-first approach fits kernel-unchanged constraints.
- **scope_clear**: YES
  - scope is v2 ABI guardrails only (not TASK-0028 learn/generator, not TASK-0188 kernel seccomp).
- **touched_paths_allowlist_present**: YES
  - TASK-0019 includes explicit allowlist for ABI/policy/selftest/docs/harness paths.

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - TASK-0028 and TASK-0188 are now explicit in TASK-0019 header.
- **security_considerations_complete**: YES
  - threat model, invariants, DON'T DO, and required negative tests include auth/spoofing/overflow rejects.

## Dependencies & blockers
- **blocked_by**: none for kickoff/prep
- **prereqs_ready**: YES
  - `TASK-0006`, `TASK-0008`, and `TASK-0009` are complete and referenced.
  - canonical QEMU harness contract exists in `scripts/qemu-test.sh`.

## Decision
- **status**: READY (TASK-0019 implementation planning/execution)
- **notes**:
  - prefer `policyd` as single policy authority; justify `abi-filterd` only if unavoidable.
  - keep marker/server wording stable via unified readiness marker (`abi-profile: ready (server=...)`).
  - lifecycle stop-condition for TASK-0019 is static boot/apply only; runtime transitions belong to TASK-0028.
  - keep deny/audit semantics deterministic and bounded (no unbounded audit spam).
  - keep kernel-seccomp claims strictly out of TASK-0019 scope.
