---
title: TASK-0032 packagefs v2: read-only package image + precomputed index (O(1) lookup) + host tooling (host-first, OS-gated)
status: In Progress
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0007
  - TASK-0009
  - TASK-0010
follow-up-tasks:
  - TASK-0033
  - TASK-0286
  - TASK-0287
  - TASK-0290
links:
  - Vision: docs/agents/VISION.md
  - Contract seed (this task): docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md
  - Packaging baseline: docs/packaging/nxb.md
  - Production gate policy: tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md
  - Depends-on (packaging drift fix): tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Depends-on (block device model): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Depends-on (MMIO access, if blk-backed on OS): tasks/TASK-0010-device-mmio-access-model.md
  - Related (VMO plumbing): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - Related (VMO splice follow-up): tasks/TASK-0033-packagefs-v2b-vmo-splice-from-image.md
  - Architecture baseline: docs/architecture/12-storage-vfs-packagefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Current `packagefsd` implementation already has a split posture:

- **host (`std_server.rs`)**: in-memory registry of published entries; path sanitize logic exists, but no
  on-disk RO image/index contract yet.
- **os-lite (`os_lite.rs`)**: fetches image bytes from `bundlemgrd` and decodes `bundleimg` entries into
  an in-memory registry; current format is functional bring-up, but not a versioned production contract.

We want packagefs to scale by serving packages from a **read-only image** with a **precomputed index**
for fast lookup.

## Goal

Introduce a **read-only package image v2** format with a deterministic precomputed index, and update
`packagefsd` to use it (host-first; OS integration gated).

Key properties:

- fast `stat/open/read` (index lookup \(O(1)\) by `(bundle, path)`),
- deterministic image build (stable ordering; reproducible hash),
- integrity checks at mount (index hash, per-file hashes optional in v2),
- keep existing `pkg:/` consumer behavior stable where possible.

## Non-Goals

- Full “blk-backed mount on OS” without a defined blk authority + proof ladder.
- Cross-process VMO splice (separate task; depends on VMO transfer semantics).
- Writable packagefs.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Deterministic format: little-endian, versioned, stable sorting.
- Bounded parsing: cap index size and reject malformed entries deterministically.
- No fake markers: “v2 mounted” only after the image was validated and index loaded.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Security section (mandatory)

### Threat model (this task)

- Corrupted or malicious package image bytes delivered to packagefs mount path.
- Path traversal payloads (`..`, empty segments, malformed UTF-8) in index/file paths.
- Out-of-range `(offset,len)` index entries causing OOB reads or silent truncation.
- Authority drift where packagefs accepts identity/policy decisions from payloads instead of channel authority.

### Security invariants

- Mount is fail-closed: invalid header/index/hash/range rejects mount deterministically.
- Parser is bounded: hard caps for image size, index bytes, entry count, path length, and file length.
- Read path is read-only and deterministic; no write semantics introduced.
- Identity/policy remains authority-bound (`sender_service_id`/service channel), never payload-derived.
- No secrets or unstable variable data in success markers.

### Required negative proofs (`test_reject_*`)

- `test_reject_pkgimg_bad_magic_or_version`
- `test_reject_pkgimg_index_hash_mismatch`
- `test_reject_pkgimg_entry_out_of_bounds`
- `test_reject_pkgimg_path_traversal_or_empty_segment`
- `test_reject_pkgimg_index_cap_exceeded`

## Red flags / decision points

- **RESOLVED (manifest format drift)**:
  - Baseline is now aligned: `docs/packaging/nxb.md` and `tools/nxb-pack` both use `manifest.nxb`.
  - `pkgimg-build` must keep `manifest.nxb` canonical and must not reintroduce `manifest.json` fallback silently.
- **YELLOW (OS storage path)**:
  - If we want OS to mount from `virtio-blk`, this is **no longer blocked by MMIO** (TASK-0010 is Done),
    but it *is* gated on selecting a single blk authority and proving deterministic reads end-to-end.
  - Short-term OS path can continue to fetch the image bytes from `bundlemgrd` (already exists), which avoids
    direct block device access in v2.
  - Exit criteria for this yellow item in TASK-0032: keep `bundlemgrd.fetch_image` as authority path and make
    blk-backed mount explicit follow-up work (do not silently scope-creep here).

## Contract sources (single source of truth)

- `docs/packaging/nxb.md` (bundle layout direction)
- `docs/architecture/12-storage-vfs-packagefs.md` (service boundary and ownership)
- `source/services/packagefsd/src/std_server.rs` (host in-memory baseline)
- `source/services/packagefsd/src/os_lite.rs` (existing `fetch_image` path and `bundleimg` decoder)
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate-C production-grade closure language)
- QEMU marker contract: `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add deterministic host tests:

- build an image from 2 small bundles (fixture),
- verify index hash matches and mount succeeds,
- `stat/open/read` resolve correct bytes with random seeks,
- corruption:
  - bad index hash → mount fails deterministically,
  - out-of-range offsets → mount fails deterministically.

### Proof (OS / QEMU) — gated

Once OS can provide the image (via existing `bundlemgrd.fetch_image` or blk-backed mount):

- `packagefsd: v2 mounted (pkgimg)`
- `SELFTEST: pkgimg mount ok`
- `SELFTEST: pkgimg stat/read ok`

## Production-grade gate mapping (TRACK alignment)

This task is part of **Gate C (Storage, PackageFS & Content)** and keeps the following production-grade
obligations *inside TASK-0032*:

- deterministic, bounded, integrity-checked image format contract,
- deterministic reject behavior for malformed/corrupt image/index/path/range inputs,
- stable `stat/open/read` semantics with reproducible proofs.

The following production-grade closure items remain explicit follow-ups and are **not** silently absorbed:

- `TASK-0033`: VMO splice/zero-copy data-path for large payload reads.
- `TASK-0286` + `TASK-0287`: kernel resource/accounting truth needed for full quota/resource claims.
- `TASK-0290`: kernel zero-copy closure truth (seal/rights/reuse/copy-fallback evidence).

## Touched paths (allowlist)

- `userspace/storage/` (new `package-image` format crate)
- `tools/pkgimg-build/` (new host tool) and optionally `tools/nxb-pack/` integration
- `source/services/packagefsd/` (v2 image-backed path; host + os-lite)
- `source/apps/selftest-client/` (gated markers)
- `docs/architecture/12-storage-vfs-packagefs.md` (new/updated for pkgimg v2)
- `docs/testing/index.md`
- `scripts/qemu-test.sh` (gated)

## Plan (small PRs)

1. **Define the on-disk format (`pkgimg`)**
   - `Superblock` (LE) with:
     - magic + version
     - index offset/len
     - data offset/len
     - `sha256(index_bytes)`
   - `Index` encoding (CBOR preferred for bounded parse) describing bundles and file entries:
     - `(bundle, version, path) -> (off,len,sha256?)`
   - 4KiB alignment for large blobs to support future VMO slicing.

2. **Host builder + verifier**
   - New tool `pkgimg-build`:
     - ingest `.nxb` directories
     - lay out blobs deterministically
     - write superblock+index+data
   - New tool `pkgimg-verify`:
     - verify index hash
     - optionally verify per-file sha256.

3. **packagefsd v2 implementation**
   - Host mode: open a file image path and build an in-memory hash map for \(O(1)\) lookups.
   - OS-lite mode:
     - continue using the existing `bundlemgrd.fetch_image` IPC path, but switch the decoded format to `pkgimg`.
   - Markers:
     - `packagefsd: v1 (mem)` for current in-memory registry
     - `packagefsd: v2 mounted (pkgimg)` for v2 image-backed.

4. **Selftest (OS-gated)**
   - Verify mount marker + read a known `pkg:/demo.hello/manifest...`.

5. **Docs**
   - `docs/architecture/12-storage-vfs-packagefs.md` describing v2 image layout, determinism, and migration strategy.

## Follow-ups (separate tasks)

- VMO splice from package image (`TASK-0033`; requires VMO transfer semantics and budgets).
- blk-backed mount on OS (TASK-0010 MMIO primitive is Done; remaining gate is choosing the blk authority + QEMU proof ladder).
- Gate-C production-grade closeout dependencies: `TASK-0286`, `TASK-0287`, `TASK-0290`.
