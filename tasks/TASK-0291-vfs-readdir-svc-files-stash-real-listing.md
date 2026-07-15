---
title: TASK-0291 VFS ReadDir + stable errors + svc.files (FILES permission, filemanager role) + stash lists real content
status: In Progress
owner: @runtime
created: 2026-07-15
depends-on: []
follow-up-tasks:
  - TASK-0292
  - TASK-0293
  - TASK-0294
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Contract seed (this task, VFS surface): docs/rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md
  - Contract seed (this task, app surface): docs/rfcs/RFC-0073-app-files-surface-svc-files-permission-filemanager-role.md
  - Role model: docs/dev/app-platform/privileged-roles.md
  - Track: tasks/TRACK-STASH-USER-DATA-FS.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

The stash file manager (`userspace/apps/stash/`) is an honest mock: six hard-coded rows, because
no `.nx` app can touch files — `vfs.capnp` has no directory listing (only Open/Read/Close/Stat/
Mount, all responses a bare `ok: Bool`), `dsl_services.capnp` has no `files` namespace, and
`SERVICE_ROUTES` has no files row or permission. This task is the **early win** of the
user-data-FS track: real listing before any new storage exists, by exposing the already-shipped
read-only packagefs mount through a proper surface.

## Goal

End-to-end, boot-proven:

1. vfsd answers `ReadDir` with bounded pagination + the RFC-0072 stable error codes (`err` field
   added to all responses).
2. `svc.files.list/stat` exists in the DSL surface, routed `files → vfsd` behind a new
   `nexus.permission.FILES` (fail-closed).
3. `filemanager` is a real bundle type with a pack-time capability ceiling (FILES only packs for
   filemanager) and is launchable.
4. stash ships as `bundle_type = "filemanager"`, its mock rows are **deleted**, and it renders a
   real listing of `/packages` (read-only) with honest empty/error states.

## Non-Goals

- Write ops / `/data` (TASK-0293 after nxfs P1).
- Mime resolution / file-type icons (TASK-0294) — rows may use the existing generic glyphs.
- Pickers or FILES for ordinary apps (deferred; RFC-0073 scope rule).
- Any change to windowd (compositor boundary, RFC-0067).

## Constraints / invariants (hard requirements)

- capnp changes additive only (append fields/structs; no renumbering).
- Bounded: ≤ 64 entries/page, name ≤ 255 bytes, reply fits the 8 KiB frame.
- Fail-closed: no FILES grant → route absent → deterministic unavailable-service error in the DSL;
  never an empty-listing fake.
- No `unwrap/expect` in services; no fake markers (listing marker counts real entries).
- Init ctrl-plane discipline: new wiring transfers go BEHIND the execd probe block (slots 11–14
  positional); named routes/@reply are persistent slots — never closed.
- Child slot for `files` = **16** (next free; 14 is the app-host events slot, 15 = settings).

## Red flags / decision points

- **YELLOW (packagefs listing semantics)**: packagefsd indexes bundle contents; ReadDir on
  `/packages` returns bundle roots, deeper paths return bundle-relative entries — exact mapping is
  decided at implementation against the existing `FileEntry` index (no new index format).
- **YELLOW (DSL result type)**: `List<FileEntry>` needs a DSL-side record type; follow the
  existing `AppEntry`/`User` precedent in `dsl_services.capnp` + runtime decoding.

## Contract sources (single source of truth)

- VFS surface + error table: RFC-0072 (Phase 1 slice).
- App surface + permission + role + slot: RFC-0073 (Phase 1 slice).
- Route table: `source/libs/nexus-sdk-routes/src/lib.rs` (one new row; consistency test extends).

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p vfsd`: ReadDir pagination determinism (exact page boundaries, eof), error-code
  negatives (`test_reject_*`: ENOTFOUND, ENOTDIR, EINVAL cursor, E2BIG limit), namespace-filtered
  listing still honored.
- `cargo test -p nexus-vfs`: client API roundtrip for readdir/stat with err mapping.
- `cargo test -p nexus-sdk-routes`: table consistency incl. `files` row (slot 16, permission
  format).
- nxb-pack test: manifest with `caps = [FILES]` + `bundle_type = "app"` → deterministic pack
  error; `bundle_type = "filemanager"` → packs.

### Proof (OS / QEMU) — required

- `vfsd: readdir ok (mount=/packages entries=<n>)`
- `app-host: svc.files routed (slot=16)`
- `stash: listing real (n=<n>)` with n ≥ 1
- `SELFTEST: files denied without cap ok` (an app without FILES gets the deny path)
- Visible-boot evidence: stash launched by click, screenshot shows real `/packages` entries
  (not the six demo names).

## Touched paths (allowlist)

- `tools/nexus-idl/schemas/vfs.capnp`, `tools/nexus-idl/schemas/dsl_services.capnp`,
  `tools/nexus-idl/schemas/manifest.capnp` (bundle type enum)
- `source/services/vfsd/` (ReadDir + err codes), `source/services/packagefsd/` (provider listing
  hook if needed)
- `userspace/nexus-vfs/` (client API)
- `source/libs/nexus-sdk-routes/` (files row)
- `tools/nxb-pack/` (filemanager mapping + FILES ceiling)
- `source/services/bundlemgrd/` (launchable), `source/services/abilitymgr/` (route provisioning
  path if any per-permission wiring is needed)
- app-host runtime (svc.files dispatch): `userspace/dsl/runtime/`, app-host service host
- `userspace/apps/stash/` (manifest + store + page)
- `source/apps/selftest-client/`, `scripts/qemu-test.sh`, `docs/storage/vfs.md`

## Plan (small PRs)

1. vfs.capnp + vfsd ReadDir + err fields + host tests.
2. nexus-vfs client + negatives.
3. manifest.capnp `filemanager` + nxb-pack ceiling + bundlemgrd launchable + tests.
4. dsl_services `files.list/stat` + routes row + app-host dispatch + deny selftest.
5. stash: manifest flip + store effect + real rows + honest empty/error states.
6. QEMU markers + visible-boot evidence.
