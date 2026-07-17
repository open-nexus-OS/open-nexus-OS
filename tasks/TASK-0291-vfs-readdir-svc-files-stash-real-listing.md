---
title: TASK-0291 VFS ReadDir + stable errors + svc.files (FILES permission, filemanager role) + stash lists real content
status: In Review
owner: @runtime
created: 2026-07-15
depends-on: []
follow-up-tasks:
  - TASK-0292
  - TASK-0293
  - TASK-0294
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
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

Marker ladder (gated, `scripts/qemu-test.sh`):

- `SELFTEST: vfs readdir ok` (root page with ≥ 1 entry over kernel IPC)
- `SELFTEST: vfs readdir deny ok` (unknown bundle → stable ENOTFOUND)

Launch-path evidence (visible boot, uart log):

- `execd: app route granted svc=files` (fail-closed provisioning fired for stash)
- `vfsd: readdir ok (mount=/packages entries=<n>)` (server-side count)
- `apphost: dsl svc files.list ok (n=<n>)` with n ≥ 1 (the DSL listing is real)
- Screenshot: stash launched by click shows real `/packages` entries (not the six demo names).

The app-without-FILES deny path is covered at pack time
(`test_reject_files_cap_for_plain_app_bundle_type`, nxb-pack) plus the fail-closed route
provisioning (no cap → no slot → `ERR_SVC_UNAVAILABLE`); a dedicated runtime deny selftest needs
a DSL test app without FILES and is deferred to the track's next slice.

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

## Progress snapshot (2026-07-15) — all phases delivered, awaiting review/commit

- [x] Shared surface crate `userspace/vfs-types` (`nexus-vfs-types`): RFC-0072 error-code SSOT +
  DirEntry bounds + the bounded raw ReadDir codec used on BOTH os-lite hops (client↔vfsd,
  vfsd↔packagefsd) — one codec, no wire drift; 11 unit tests incl. byte-budget truncation.
- [x] packagefsd: shared `listing.rs` (canonical order, synthesized dirs, ENOTFOUND/ENOTDIR) wired
  into BOTH servers (os_lite raw `OPCODE_LIST=4`, std capnp `ListPath/ListResponse`); client
  `PackageFsClient::list`.
- [x] vfsd: `OPCODE_READDIR=6` in BOTH servers; std path with `FsProvider::read_dir` + `err` codes
  on every capnp response; os-lite path validates-and-relays provider pages.
- [x] nexus-vfs client `read_dir` (os raw + host capnp) + `Error::Vfs(code)`.
- [x] Platform: `svc.files.list/stat` (dsl_services), `files→vfsd` route slot 16, FILES in
  abilitymgr KNOWN_PERMISSIONS, `filemanager @8` bundle type + nxb-pack ceiling + bundlemgrd
  launchable, app-host `files.*` arms (bounded 7 KiB reply scratch, human `sizeText`).
- [x] stash: `bundle_type = "filemanager"`, caps = WINDOW+FILES (SETTINGS dropped —
  `window.control` rides the windowd presentation channel), mock rows deleted, root-effect
  initial load, honest loading/error states, breadcrumb shows the real path.
- [x] Three plumbing fixes found by the boot proof (recorded in the wiring):
  init RouteTable is requester-scoped → `(Execd, Vfsd)` in REQUIRED_ROUTES + execd wiring arm;
  `KernelClient::recv` truncates at 512 B → `recv_into` with 8 KiB scratch on the packagefsd hop;
  vfsd replies now ride the caller's CAP_MOVE reply inbox (`recv_request_with_meta` + ReplyCap,
  settingsd pattern) so app-host children actually receive them.

## Proof evidence (closure run 2026-07-15)

- Host: `cargo test -p nexus-vfs-types -p packagefsd -p vfsd -p nexus-vfs -p nexus-packagefs
  -p nexus-sdk-routes -p abilitymgr -p nxb-pack -p vfs-e2e -p nexus-init` — all green, incl. the
  new e2e (`tests/vfs_e2e/tests/readdir_e2e.rs`: pagination determinism + error codes end-to-end)
  and `test_reject_files_cap_for_plain_app_bundle_type` (pack-time ceiling deny).
- OS marker ladder: `scripts/qemu-test.sh --profile=headless` exit 0 —
  `SELFTEST: vfs readdir ok`, `SELFTEST: vfs readdir deny ok` verified in sequence.
- Visible boot (virgl, VNC-driven): login → click stash →
  `execd: app route granted svc=files`, `vfsd: readdir ok (mount=/packages entries=3)`,
  `apphost: dsl svc files.list ok (n=3)`; screenshots show the real `/packages` listing
  (demo.exit0 / demo.hello / system) and full selection reactivity (accent row + properties
  pane Name/Type from real data + Move/Copy/Share/Delete action bar).
