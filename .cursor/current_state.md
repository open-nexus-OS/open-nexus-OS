# Cursor Current State (SSOT)

<!--
CONTEXT
This file is the single source of truth for the current system state.
Keep it compact, explicit, and contract-oriented.
-->

## Current architecture state
- **last_decision**: refine `TASK-0019` into explicit phased rollout (including dedicated profile-distribution phase), sharpen marker/server wording, and lock policy-lifecycle boundary; set `TASK-0017` + `TASK-0018` to `Done`.
- **rationale**:
  - ABI guardrails remain kernel-unchanged and are delivered in bounded phases instead of one large all-services cutover,
  - profile distribution needs explicit/authenticated scope as its own delivery phase,
  - lifecycle split between static TASK-0019 and runtime TASK-0028 must stay explicit.
- **active_constraints**:
  - kernel untouched in this slice,
  - this is not a hard security boundary against raw `ecall` bypasses,
  - profile parsing/matching must be bounded + deterministic,
  - deny decisions must be auditable and fail-closed,
  - subject identity must be kernel-derived (`service_id` / `sender_service_id`), never payload text.

## Current focus (execution)
- **active_task**: `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md` (In Progress, phased rollout execution)
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
- **phase_now**: TASK-0019 is now in progress with phased execution model (A-F) and lifecycle stop-condition boundary to `TASK-0028`.
- **baseline_commit**: `9ef6e59` (latest committed baseline before TASK-0019 prep)
- **next_task_slice**:
  - lock implementation approach for profile authority (`policyd`-first vs `abi-filterd` fallback),
  - define deterministic profile format/bounds + authenticated distribution checks,
  - add required `test_reject_*` and marker plan without expanding into v2 argument-learning scope.

## Last completed
- `TASK-0018` handoff was archived for continuity:
  - archive: `.cursor/handoff/archive/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`
  - status: done with completed proofs and closeout commits.
- `TASK-0017` status is now `Done` (closed proof package already present in task/board evidence).

## Proof baseline currently green
- `TASK-0017` closure baseline remains green (host + single-VM + 2-VM evidence recorded).
- `TASK-0018` closure baseline remains green (`just dep-gate`, `just diag-os`, single-VM `qemu-test.sh`).
- `TASK-0019` proof commands are not run yet (prep phase only).

## Active invariants (must hold)
- **security**
  - deny-by-default syscall profiles for compliant binaries,
  - authenticated profile ingestion with deterministic rejects for spoofing/overflow,
  - clear documentation that this v2 is guardrail/hygiene, not kernel sandboxing.
- **determinism**
  - stable deny/error labels and bounded profile parsing/matching,
  - bounded audit emission (no unbounded deny spam loops).
- **scope hygiene**
  - keep TASK-0019 separate from TASK-0028 (`learn/enforce` generator scope) and TASK-0188 (kernel seccomp).
  - keep TASK-0019 lifecycle static (boot-time apply) and defer runtime lifecycle transitions to TASK-0028.

## Open threads / follow-ups
- ABI-filter follow-ons (do not absorb now):
  - `tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md`
  - `tasks/TASK-0188-kernel-sysfilter-v1-task-profiles-rate-buckets.md`

## DON'T DO (session-local)
- DON'T claim ABI filter v2 is a hard sandbox against malicious raw `ecall`.
- DON'T accept profile authority/subject identity from payload strings.
- DON'T add unbounded matcher semantics or unbounded audit output paths.
- DON'T silently expand into `TASK-0028` learn/generator or `TASK-0188` kernel scope.
