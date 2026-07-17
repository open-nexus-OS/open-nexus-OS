# Storage: `vfsd` + `packagefsd` + `statefsd` (+ `nxfsd`) — onboarding

Storage follows **one authority per store**, with `vfsd` as the single client-facing surface
(ADR-0043):

| namespace | authority | nature |
|---|---|---|
| `/packages` | `packagefsd` | read-only bundle content (pkgimg v2) |
| `/state` | `statefsd` | boot-critical service-state KV (NOT files) — `docs/storage/statefs.md` |
| `/data` | `nxfsd` (planned, contract seeded) | writable user-data filesystem — `docs/storage/nxfs.md` |

Canonical sources:

- VFS overview: `docs/storage/vfs.md`
- StateFS current state + hardening roadmap: `docs/storage/statefs.md`
- nxfs orientation (user data, `/data`): `docs/storage/nxfs.md`
- Track (milestones to a working stash file manager): `tasks/TRACK-STASH-USER-DATA-FS.md`
- Packaging and publication: `docs/packaging/nxb.md`
- Service architecture context: `docs/adr/0017-service-architecture.md`
- Block topology (GPT, single device owner): `docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md`
- Testing guide + marker discipline: `docs/testing/README.md` and `scripts/qemu-test.sh`

## Responsibilities

### `packagefsd`

- Tracks installed bundles (published by `bundlemgrd` after successful install).
- Exposes read-only content under `/packages/<name>@<version>/...`.
- Maintains the active alias mapping used by `pkg:/<name>/...`.

### `vfsd`

- Exposes the Cap’n Proto VFS interface to clients.
- Holds a mount table and routes lookups to file system providers.
- v1 surface: open/read/stat/close, read-only. **VFS v2 (RFC-0072)** adds `ReadDir` (bounded
  pagination), a stable numeric error-code SSOT on every response, and — per-mount `writable` —
  write ops (create/write/truncate/mkdir/rename/remove) for the `/data` provider; RO mounts keep
  rejecting writes (`EROFS`), now deterministically coded.
- Per-app confinement (namespaces + CapFd, RFC-0042) applies to all ops, old and new.

### `statefsd` / `nxfsd`

- `statefsd` is NOT reachable through vfsd — the `/state` KV has its own capability-gated protocol
  (see `docs/storage/statefs.md`). Keep the boundary: no file semantics in statefs (ADR-0043).
- `nxfsd` (contract RFC-0071, bring-up TASK-0293) registers as vfsd's first writable provider at
  `/data`; apps reach it only via `svc.files.*` behind `nexus.permission.FILES`
  (filemanager-role-gated, RFC-0073).

## Client boundary

Clients should use the `nexus-vfs` crate.

- Host tests can use loopback wiring.
- OS builds route over kernel IPC.

## Proof and markers

Storage/VFS behavior is proven in two layers:

- Host integration tests (`tests/vfs_e2e`, related harnesses)
- QEMU smoke run marker contract (`scripts/qemu-test.sh`)

When you change path semantics, aliasing, or error behavior:

- update the owning task(s) stop conditions,
- update host E2E tests,
- and update the canonical QEMU harness expectations if and only if the real behavior changed.

## PackageFS v2 `pkgimg` contract

`TASK-0032` / `RFC-0041` introduce a deterministic read-only image contract for package content:

- `pkgimg` v2 superblock contains magic/version, index/data offsets and lengths, and
  `sha256(index_bytes)`.
- `packagefsd` must validate header, bounds, and index hash before exposing mount success.
- Invalid version/magic/hash/range/path data is fail-closed.
- Path traversal (`..`, empty segments) is rejected during parse.

Current runtime path ownership:

- Host path (`packagefsd` std mode) can mount a v2 image from `PACKAGEFSD_PKGIMG_PATH`.
- OS-lite path continues to fetch image bytes from `bundlemgrd.fetch_image`; the decode contract is now
  `pkgimg` v2 (not legacy `bundleimg`).
- VMO splice/zero-copy read data path stays explicitly out-of-scope here; it is tracked in
  `TASK-0295` (RFC-0072 Phase 3 — the seam moved to the vfsd surface so packagefs and nxfs share
  it; the older `TASK-0033` is superseded).
