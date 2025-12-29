---
title: TASK-0132 Storage errors: strict cross-service error semantics contract (vfs/content/fileops/trash/grants) + tests
status: Draft
owner: @platform
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Content providers baseline: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Scoped grants enforcement: tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - File operations + trash: tasks/TASK-0085-ui-v12b-fileops-trash-services.md
  - Persistence substrate (state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
---

## Context

Today we have many storage-facing services (`contentd`, `vfsd`, `fileopsd`, `trashd`, `grantsd`) and UI
flows that rely on predictable errors. Without a shared error contract, clients drift into ad-hoc
string reasons and inconsistent behaviors.

This task introduces a strict, portable error semantics contract surfaced through Cap’n Proto and
enforced by deterministic host tests.

## Goal

Deliver:

1. A shared error enum contract (Cap’n Proto):
   - `tools/nexus-idl/schemas/vfs_errors.capnp` (or equivalent canonical location)
   - includes a minimal stable set of errors used across storage surfaces:
     - `ENOENT`, `EEXIST`, `EISDIR`, `ENOTDIR`, `EPERM`, `EROFS`, `ENOSPC`, `EBUSY`, `EXDEV`, `EINVAL`
     - **and** `EDQUOTA` if quotas are enforced (see `TASK-0043`)
2. A shared mapping layer:
   - `contentd` providers map backend errors → `vfs_errors::Err`
   - `fileopsd` and `trashd` propagate errors precisely and deterministically
   - grants failures must be `EPERM` (or a dedicated “EACCES” if added later; v1 uses `EPERM`)
3. Deterministic host tests:
   - exercise each error from at least one provider implementation (or a test-only provider)
   - ensure no unknown/“stringly typed” errors leak across the IDL boundary
4. Documentation:
   - a single table describing when each error must be returned

## Non-Goals

- Kernel changes.
- A full POSIX errno surface. v1 stays minimal and stable.

## Constraints / invariants (hard requirements)

- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: tests must validate exact error codes, not log text.

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic tests (suggested: `tests/storage_errors_host/`):

- each error code is produced deterministically from a known operation
- cross-service flows preserve the code (e.g., `contentd` → `fileopsd` propagation)

## Touched paths (allowlist)

- `tools/nexus-idl/schemas/vfs_errors.capnp` (new)
- `source/services/contentd/` (map errors)
- `source/services/fileopsd/` (propagate)
- `source/services/trashd/` (propagate)
- `source/services/grantsd/` (propagate)
- `tests/` (new host tests)
- `docs/storage/errors.md` (new)

## Plan (small PRs)

1. Define `vfs_errors.capnp` + mapping helpers in a shared crate/module
2. Wire mapping into `contentd` provider(s)
3. Wire propagation into `fileopsd`/`trashd`/`grantsd`
4. Add host tests + docs
