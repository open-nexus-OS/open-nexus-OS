# Current Handoff: TASK-0019 ABI syscall guardrails prep

**Date**: 2026-03-26  
**Status**: implementation in progress (phased rollout + lifecycle boundary sharpened; TASK-0017/0018 marked done).  
**Contract baseline**: `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md` (`In Progress`)

---

## What is stable now

- `TASK-0018` handoff is archived:
  - `.cursor/handoff/archive/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`
- `TASK-0017` and `TASK-0018` are now marked `Done` in task headers and queue boards.
- `TASK-0019` header now includes explicit follow-up boundaries:
  - `TASK-0028` (ABI filters v2 learn/enforce + generator),
  - `TASK-0188` (kernel-level syscall enforcement boundary).
- `TASK-0019` links are aligned to current policy/audit contracts (`RFC-0015`, `TASK-0006`, `TASK-0008`, `TASK-0009`).
- Security concerns for `TASK-0019` are strengthened:
  - authenticated profile distribution,
  - subject-binding rejects,
  - bounded rule-count/path/argument parsing requirements.
- TASK-0019 execution shape is now explicit multi-phase rollout:
  - staged migration toward "all shipped OS components",
  - dedicated profile-distribution phase,
  - static lifecycle stop-condition (runtime transitions deferred to TASK-0028).
- Queue boards now include `TASK-0019` (`IMPLEMENTATION-ORDER`, `STATUS-BOARD`).

## Current focus

- keep TASK-0019 contract-first and drift-free before implementation begins.

## Relevant contracts and linked work

- Active task:
  - `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`
- RFC / architecture baseline:
  - `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md`
  - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
  - `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`
- Dependency contracts:
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
  - `tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md`
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
- Follow-up boundaries (out of this slice):
  - `tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md`
  - `tasks/TASK-0188-kernel-sysfilter-v1-task-profiles-rate-buckets.md`
- Testing contract:
  - `scripts/qemu-test.sh`
  - `docs/testing/index.md`

## Immediate next slice

1. Lock source-of-truth decision for profile authority (`policyd` preferred, `abi-filterd` only with explicit justification).
2. Start Phase A implementation (bounded filter chain + deterministic deny/audit for selected syscall set).
3. Prepare phase-by-phase rollout evidence plan through Phase F without scope leak into TASK-0028/TASK-0188.

## Guardrails

- Keep kernel untouched.
- Keep messaging explicit: v2 guardrail for compliant binaries, not a malicious-code sandbox.
- Keep identity/auth checks kernel-derived (`service_id`/`sender_service_id`), never payload strings.
- Keep parsing/matching bounded and deterministic; deny decisions auditable.
- Keep TASK-0019 lifecycle static (boot/startup apply only); runtime lifecycle transitions belong to TASK-0028.
- Keep task scope separate from v2 learn/generator (`TASK-0028`) and kernel seccomp (`TASK-0188`).

## Proof snapshot

- This slice is prep/alignment only; no TASK-0019 implementation proofs executed yet.
