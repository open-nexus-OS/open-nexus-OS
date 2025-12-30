---
title: TASK-0135 Storage UI + nx-state: quotas/snapshots/GC controls in Settings + CLI helpers + markers/tests/docs
status: Draft
owner: @ui
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - SystemUI→DSL Settings baseline: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - SystemUI→DSL OS wiring: tasks/TASK-0122-systemui-dsl-migration-phase2b-os-wiring-postflight-docs.md
  - StateFS snapshots/compaction: tasks/TASK-0134-statefs-v3-snapshots-compaction-mounts.md
  - State quotas: tasks/TASK-0133-statefs-quotas-v1-accounting-enforcement.md
---

## Context

Storage management is a user-facing requirement once `/state` is real:

- users need to see per-app usage,
- adjust quotas (where allowed),
- create/manage snapshots,
- and run GC/compaction (admin).

We also want a small CLI (`nx-state`) for developer workflows and deterministic proofs.

## Goal

Deliver:

1. Settings → Storage (DSL) page:
   - global summary (used/free, reserve if applicable)
   - per-app usage list (logical/physical where available)
   - quota controls (set soft/hard)
   - snapshot controls (create/list/mount/unmount/delete where supported)
   - “Run GC now” (admin-gated)
   - markers:
     - `settings:storage open`
     - `storage:quota set app=...`
     - `storage:snapshot create app=... name=...`
2. CLI `tools/nx-state`:
   - `nx state quota get/set`
   - `nx state snap create/list/mount`
   - `nx state gc`
   - output designed for deterministic parsing (stable ordering, no timestamps unless explicit)
3. Deterministic host tests:
   - UI model tests for quota/snapshot actions using mocks
   - CLI parsing tests and stable output goldens

## Non-Goals

- Kernel changes.
- Full privileged admin model (v1 can gate admin actions behind a build flag or a policy stub, but must be explicit).

## Constraints / invariants (hard requirements)

- A11y labels/roles for all controls.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- deterministic host tests prove UI actions call the correct bridge/service methods

### Proof (OS/QEMU) — gated

- Storage page opens and emits markers
- quota/snapshot actions emit markers and reflect state

## Touched paths (allowlist)

- `userspace/systemui/dsl/pages/settings/Storage.nx` (new)
- `tools/nx-state/` (new)
- `docs/systemui/storage.md` (new)
- `docs/storage/` (updated)

## Plan (small PRs)

1. DSL Storage page (host-first) + a11y + markers
2. nx-state CLI helpers + stable output
3. host tests + docs + OS markers (gated)

