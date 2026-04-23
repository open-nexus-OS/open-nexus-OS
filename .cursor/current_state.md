# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-04-23)**: `TASK-0032` closure is synchronized to implementation reality.
  - `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` is now `Done`.
  - `docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md` is now `Done`.
  - Host + OS proofs are recorded (including required pkgimg marker ladder).
- **preserved boundary**: Gate-C follow-up split remains explicit and unchanged:
  - `TASK-0033` owns VMO splice/zero-copy data-path.
  - `TASK-0286`/`TASK-0287`/`TASK-0290` own kernel production closure truths.
- **new decision (2026-04-23)**: `TASK-0039` and `RFC-0042` are now both `In Progress` under Gate B execution.

## Active focus (execution)

- **active_task**: `tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md` — `In Progress`
- **active_contract**: `docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md` — `In Progress`
- **recently_closed**: `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` — `Done`
- **tier_target**: production-grade trajectory per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate B: Security, Policy & Identity)

## Active constraints (for TASK-0039 readiness)

- Kernel scope remains untouched unless explicitly owned by follow-up kernel tasks.
- No fake-success markers; success only after real behavior/proofs.
- Keep userspace sandbox boundary claims honest (no kernel-enforced claims in v1 scope).
- Preserve deterministic reject taxonomy (`test_reject_*`) for traversal/capfd/namespace denials.

## Contract links (active)

- `tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md`
- `docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md`
- `tasks/TASK-0043-security-v2-sandbox-quotas-egress-abi-audit.md`
- `tasks/TASK-0189-sandbox-profiles-v2-sandboxd-or-policyd-distribution-ipc-vfs.md`
- `source/services/vfsd/src/os_lite.rs`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`

## Carry-over note

- `TASK-0023B` external CI replay artifact closure remains independent and non-blocking for Gate-C packagefs execution.
