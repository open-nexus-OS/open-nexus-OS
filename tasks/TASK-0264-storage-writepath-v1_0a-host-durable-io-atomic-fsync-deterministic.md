---
title: TASK-0264 Storage Write-Path v1.0a (host-first): durable I/O + atomic operations + fsync barriers + crash-recovery + deterministic tests
status: Draft
owner: @platform
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Content provider foundations: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Content quotas: tasks/TASK-0232-content-v1_2a-host-content-quotas-versions-naming-nx-content.md
  - Persistence: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic storage write-path foundation:

- durable I/O (atomic create/replace, temp-file commit),
- fsync barriers (directory + file),
- crash-recovery semantics,
- power-fail simulation.

The prompt proposes durable write semantics. `TASK-0081` already plans `contentd` service with stream handles. `TASK-0232` already plans content quotas. This task delivers the **host-first core** (durable I/O, atomic operations, fsync barriers, crash-recovery) that can be reused by both OS/QEMU integration and host tests.

## Goal

Deliver on host:

1. **Durable I/O library** (`userspace/libs/durable-io/`):
   - atomic create/replace: write-ahead temp-then-commit pattern (`open(O_CREAT|O_EXCL)`, write, **fsync(file)**, rename, **fsync(parent)`)
   - temp-file commit protocol: `create2(..., temp=true)` → creates `<name>.nxpart` hidden from listing; `commit(tempUri, ..., replace)` → atomic rename; if `replace=true`, must guarantee **atomic replace**
   - fsync barriers: `fsync(uri)` → flush file; `dirsync(parent)` → flush directory entry
   - deterministic semantics (stable given the same inputs)
2. **Crash-recovery library** (`userspace/libs/crash-recovery/`):
   - journaling stub: maintain journal in `state:/content/journal/` that records `prepare/commit/done` entries for recovery
   - recovery pass: scan journal, remove orphaned `*.nxpart` older than N minutes, finalize **committed but undirsync'd** entries (rename+dirsync)
   - idempotence/determinism
3. **Power-fail simulator library** (`tools/crashsim/content_failpoints.rs`):
   - expose **failpoints** corresponding to schema `crash_sim.points` (after_create, after_write, after_fsync, after_dirsync)
   - deterministic seeds and run order; artifact logs to `state:/crashsim/`
4. **Host tests** proving:
   - atomic replace: write A, then replace with B; on simulated crash at each failpoint, post-recovery yields either A or B, never partial
   - temp budget: exceed `temp_ratio_pct` → operation denied with specific error
   - quota enforcement: write beyond per-app bytes fails predictably
   - dirsync guarantees: after `commit+dirsync`, listing always shows the new entry

## Non-Goals

- OS/QEMU integration (deferred to v1.0b).
- Real hardware (QEMU/virtio-blk only).
- Full filesystem journaling (this is a stub for recovery pass only).

## Constraints / invariants (hard requirements)

- **No duplicate content authority**: This task provides durable I/O library. `TASK-0081` already plans `contentd` service. This task should extend `contentd` with durable write semantics, not create a parallel content service.
- **No duplicate quota authority**: This task enforces quotas at write time. `TASK-0232` already plans content quotas. Both should share the same quota enforcement to avoid drift.
- **Determinism**: durable I/O, atomic operations, fsync barriers, and crash-recovery must be stable given the same inputs.
- **Bounded resources**: temp budget is bounded; quota enforcement is bounded.
- **Persistence gating**: journaling requires `/state` (`TASK-0009`) or equivalent. Without `/state`, journaling must be disabled or explicit `stub/placeholder` (no "written ok" claims).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (content authority drift)**:
  - Do not create parallel content services. This task provides durable I/O library. `TASK-0081` (contentd) should use this library to avoid drift.
- **RED (quota authority drift)**:
  - Do not create parallel quota enforcement. This task enforces quotas at write time. `TASK-0232` (content quotas) should share the same quota enforcement to avoid drift.
- **YELLOW (atomic replace determinism)**:
  - Atomic replace must use atomic rename over same filesystem; keep old file on crash. Document the filesystem requirements explicitly.

## Contract sources (single source of truth)

- Testing contract: `scripts/qemu-test.sh`
- Content provider foundations: `TASK-0081` (contentd service)
- Content quotas: `TASK-0232` (content quotas)
- Persistence: `TASK-0009` (prerequisite for `/state`)

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p storage_writepath_v1_0_host` green (new):

- atomic replace: write A, then replace with B; on simulated crash at each failpoint, post-recovery yields either A or B, never partial
- temp budget: exceed `temp_ratio_pct` → operation denied with specific error
- quota enforcement: write beyond per-app bytes fails predictably
- dirsync guarantees: after `commit+dirsync`, listing always shows the new entry

## Touched paths (allowlist)

- `userspace/libs/durable-io/` (new)
- `userspace/libs/crash-recovery/` (new)
- `tools/crashsim/content_failpoints.rs` (new)
- `schemas/content_v1_3.schema.json` (new)
- `tests/storage_writepath_v1_0_host/` (new)
- `docs/storage/writepath_v1_0.md` (new, host-first sections)

## Plan (small PRs)

1. **Durable I/O library**
   - atomic create/replace
   - temp-file commit protocol
   - fsync barriers
   - host tests

2. **Crash-recovery library**
   - journaling stub
   - recovery pass
   - host tests

3. **Power-fail simulator**
   - failpoints
   - deterministic harness
   - host tests

4. **Schema + docs**
   - content_v1_3.schema.json
   - host-first docs

## Acceptance criteria (behavioral)

- Atomic replace: write A, then replace with B; on simulated crash at each failpoint, post-recovery yields either A or B, never partial.
- Temp budget: exceed `temp_ratio_pct` → operation denied with specific error.
- Quota enforcement: write beyond per-app bytes fails predictably.
- Dirsync guarantees: after `commit+dirsync`, listing always shows the new entry.
