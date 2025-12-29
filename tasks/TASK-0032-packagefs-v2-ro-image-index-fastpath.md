---
title: TASK-0032 packagefs v2: read-only package image + precomputed index (O(1) lookup) + host tooling (host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Packaging baseline: docs/packaging/nxb.md
  - Depends-on (packaging drift fix): tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Depends-on (block device model): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Depends-on (MMIO access, if blk-backed on OS): tasks/TASK-0010-device-mmio-access-model.md
  - Related (VMO plumbing): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Current `packagefsd` implementations:

- **host (`std_server.rs`)**: stores bundle file bytes in memory (not scalable).
- **os-lite (`os_lite.rs`)**: has an explicit TODO note to move to a “read-only bundle image”, and already
  fetches a `bundleimg` blob from `bundlemgrd` (`encode_fetch_image` / `decode_fetch_image_rsp`).

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

- Full “blk-backed mount on OS” without MMIO access (gated on TASK-0010).
- Cross-process VMO splice (separate task; depends on VMO transfer semantics).
- Writable packagefs.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Deterministic format: little-endian, versioned, stable sorting.
- Bounded parsing: cap index size and reject malformed entries deterministically.
- No fake markers: “v2 mounted” only after the image was validated and index loaded.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (manifest format drift)**:
  - Docs say `.nxb` contains `manifest.nxb`, but `tools/nxb-pack` currently writes `manifest.json`.
  - The image builder must not cement `manifest.json` as canonical. It must either:
    - operate on `manifest.nxb` only, or
    - explicitly support both formats during a transition and document it.
- **YELLOW (OS storage path)**:
  - If we want OS to mount from `virtio-blk`, that is blocked by the MMIO access model (TASK-0010).
  - Short-term OS path can continue to fetch the image bytes from `bundlemgrd` (already exists), which
    avoids block device access in v2.

## Contract sources (single source of truth)

- `docs/packaging/nxb.md` (bundle layout direction)
- `source/services/packagefsd/src/os_lite.rs` (existing `fetch_image` path and `bundleimg` decoder)
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

## Touched paths (allowlist)

- `userspace/storage/` (new `package-image` format crate)
- `tools/pkgimg-build/` (new host tool) and optionally `tools/nxb-pack/` integration
- `source/services/packagefsd/` (v2 image-backed path; host + os-lite)
- `source/apps/selftest-client/` (gated markers)
- `docs/storage/packagefs.md` (new/updated)
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
   - `docs/storage/packagefs.md` describing v2 image layout, determinism, and migration strategy.

## Follow-ups (separate tasks)

- VMO splice from package image (requires VMO transfer semantics and budgets).
- blk-backed mount on OS once virtio-blk MMIO access exists (TASK-0010).
