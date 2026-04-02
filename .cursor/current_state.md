# Cursor Current State (SSOT)

<!--
CONTEXT
This file is the single source of truth for the current system state.
Keep it compact, explicit, and contract-oriented.
-->

## Current architecture state
- **last_decision**: close `TASK-0019` as `Done` after green host/OS/QEMU proofs while keeping `RFC-0032` complete.
- **rationale**:
  - maintain kernel-unchanged boundary while adding deterministic userspace guardrails,
  - keep policy authority single-source (`policyd` + `recipes/policy`),
  - prove stop-condition markers and required `test_reject_*` set before review.
- **active_constraints**:
  - kernel untouched in this slice,
  - not a hard boundary against raw `ecall` bypasses,
  - profile parsing/matching bounded and deterministic,
  - deny decisions fail-closed and auditable,
  - subject identity is kernel-derived (`service_id` / `sender_service_id`), never payload text.

## Current focus (execution)
- **active_task**: `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md` (Done)
- **seed_contract**:
  - `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`
  - `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md`
  - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
  - `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`
  - `tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md` (follow-on boundary)
- **contract_dependencies**:
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
  - `tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md`
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
  - `scripts/qemu-test.sh`
  - `docs/testing/index.md`
- **phase_now**: TASK-0019 closeout complete and marked Done; next queue head is TASK-0020.
- **baseline_commit**: `2c76971` (user-declared baseline before this implementation slice)
- **next_task_slice**:
  - start TASK-0020 planning in strict sequential order,
  - preserve lifecycle/runtime follow-on scope in TASK-0028 and kernel boundary in TASK-0188,
  - keep TASK-0019/RFC-0032 artifacts stable as completed baseline.

## Last completed
- `TASK-0018` handoff remains archived:
  - archive: `.cursor/handoff/archive/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`
  - status: done with completed proofs and closeout commits.
- `TASK-0017` remains `Done`.

## Proof baseline currently green
- `TASK-0017` closure baseline remains green.
- `TASK-0018` closure baseline remains green.
- `TASK-0019` closure proofs green:
  - `cargo test -p nexus-abi -- reject --nocapture`
  - `cargo test -p policyd abi_profile_get_v2 -- --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - markers observed: `abi-profile: ready (server=policyd|abi-filterd)`, `abi-filter: deny (subject=selftest-client syscall=statefs.put)`, `SELFTEST: abi filter deny ok`, `SELFTEST: abi filter allow ok`, `abi-filter: deny (subject=selftest-client syscall=net.bind)`, `SELFTEST: abi netbind deny ok`.

## Active invariants (must hold)
- **security**
  - deny-by-default profiles for compliant binaries,
  - authenticated distribution + deterministic reject paths,
  - explicit non-sandbox messaging for raw `ecall`.
- **determinism**
  - stable deny/error labels and bounded parser/matcher cost,
  - bounded marker/audit emission.
- **scope hygiene**
  - keep TASK-0019 separate from TASK-0028 and TASK-0188,
  - keep TASK-0019 lifecycle static (boot/startup apply only).

## Open threads / follow-ups
- `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- `tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md`
- `tasks/TASK-0188-kernel-sysfilter-v1-task-profiles-rate-buckets.md`

## DON'T DO (session-local)
- DON'T claim ABI filter v2 is a hard sandbox against malicious raw `ecall`.
- DON'T accept profile authority/subject identity from payload strings.
- DON'T add unbounded matcher semantics or unbounded audit output paths.
- DON'T silently expand into `TASK-0028` or `TASK-0188` scope.
