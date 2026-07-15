---
title: TASK-0295 Zero-copy storage data plane: VMO splice reads (packagefs + nxfs) + VMO-backed writes + inline cap enforcement
status: In Review
owner: @runtime
created: 2026-07-15
depends-on:
  - TASK-0293
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Contract seed (this task): docs/rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md (Phase 3)
  - Data-plane substrate: docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md
  - Store contract (INLINE_IO_MAX rule): docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md
  - Supersedes: tasks/TASK-0033-packagefs-v2b-vmo-splice-from-image.md
  - Kernel production closure (soft dependency): tasks/TASK-0290 (VMO sealing)
  - Track: tasks/TRACK-STASH-USER-DATA-FS.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

All storage reads copy bytes through 8 KiB IPC frames today; RFC-0071/0072 promise the VISION
data-plane rule instead: control via Cap'n Proto, bulk bytes via VMO handles (`nexus-vmo`,
RFC-0040). TASK-0033 sketched this for packagefs alone; it is superseded — the seam belongs at the
VFS surface so packagefs AND nxfs serve the same contract.

## Goal

- RFC-0072 Phase 3 ops: VMO-handle variants for bulk read/write on the vfsd surface; providers
  fill/consume VMOs (packagefs: from pkgimg ranges; nxfs: extent IO).
- Enforce `INLINE_IO_MAX = 4096`: inline Data beyond the cap → `E2BIG` (announced in RFC-0072,
  enforced here).
- Copy-fallback stays available and **counted** (nexus-vmo `copy_fallback_count`) — perf honesty,
  no silent degradation.
- Deterministic perf floor markers (bytes moved, fallback count), not vague "faster".

## Non-Goals

- Kernel sealing/write-map-denial guarantees (TASK-0290 owns; until then the contract documents
  the trust boundary honestly).
- mmap-style shared mappings into apps; DSL apps still get bounded string surfaces (RFC-0073).

## Constraints / invariants (hard requirements)

- Fallback is explicit and counted; a build where splice silently never engages must fail its
  proof (counter assertions in selftests).
- Bounded VMO sizes per request; handle lifetime rules per RFC-0040 typed-ownership discipline.
- No provider-specific client API: one vfsd surface, two providers behind it.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- vfsd + provider tests: splice roundtrip byte-equality vs copy path, cap enforcement (`E2BIG`),
  fallback counter behavior, handle-lifetime negatives (`test_reject_*`).

### Proof (OS / QEMU) — required

- `vfsd: vmo splice read ok (bytes=<n>, fallbacks=<m>)`
- `SELFTEST: vfs splice roundtrip ok`
- `SELFTEST: vfs inline oversize deny ok`

## Touched paths (allowlist)

- `tools/nexus-idl/schemas/vfs.capnp` (Phase 3 ops, additive)
- `source/services/vfsd/`, `source/services/packagefsd/`, `source/services/nxfsd/`
- `userspace/nexus-vfs/`, `userspace/memory/` (nexus-vmo, only if counters/helpers are missing)
- `source/apps/selftest-client/`, `scripts/qemu-test.sh`, `docs/storage/vmo.md`

## Progress snapshot (2026-07-15) — read path + inline cap DONE, boot-proven

v1 delivers the **VMO-splice READ** data plane and the inline cap, modeled on the live
execd→bundlemgrd payload handoff (CAP_MOVE + header-last). VMO-backed *writes* are a follow-up
(the write surface today is bounded inline text, RFC-0073 v1).

- [x] **Shared codec** `nexus-vfs-types::splice`: `OP_READ_VMO = 7`, `INLINE_IO_MAX = 4096`,
  the 16-byte splice header (magic `NXVR`, status, len; data at offset 16),
  `encode/decode_read_vmo_request`, and `splice_fits(payload, capacity)` — the shared E2BIG
  decision. 7 host tests (header pending/ready roundtrip, fill→readback byte-equality,
  oversize→E2BIG, capacity-below-header).
- [x] **vfsd** `handle_read_vmo`: takes the CAP_MOVE'd VMO, resolves the path (nxfs `/data`
  via `DataStore::read_bytes`, or read-only `pkg:/` via the namespace), writes the payload
  FIRST + the header LAST (release ordering), closes the cap. Honest accounting: total bytes +
  fallback count in `vfsd: vmo splice read ok (bytes=<n>, fallbacks=<m>)`. Inline `OP_READ`
  above `INLINE_IO_MAX` → E2BIG sentinel (never truncated).
- [x] **nexus-vfs client** `read_vmo(path, cap)`: creates a VMO, CAP_MOVEs a clone via
  `send_with_cap_move_wait`, polls the header (bounded), reads the bytes back; maps the inline
  oversize sentinel to `Error::Vfs(TooBig)`. OS-only (host returns `Unsupported`).
- [x] **selftest** (`os_lite/vfs.rs`): splice byte-equality vs the inline read + inline oversize
  deny; two new markers declared in the proof manifest.

### Proof (Host) — met

- `cargo test -p nexus-vfs-types` (23, incl. 7 splice: fill→readback byte-equality, E2BIG via
  `splice_fits`, header pending/ready). Fallback-counter + handle-lifetime negatives are covered
  by `nexus-vmo`'s `host_contract`/`reject_contract` (the VMO transfer SSOT, RFC-0040).

### Proof (OS / QEMU) — met (headless marker ladder, `just test-os headless`, exit 0)

- `vfsd: vmo splice read ok (bytes=19, fallbacks=0)` — the splice engaged (19 B = `build.prop`),
  zero fallbacks (a real cross-process cap-move, not a copy).
- `SELFTEST: vfs splice roundtrip ok` — the spliced bytes equal the inline read, cross-process.
- `SELFTEST: vfs inline oversize deny ok` — an inline read above `INLINE_IO_MAX` is `E2BIG`.
- Full ladder `SELFTEST: Completed (markers verified)` — no regression.

### Deferred (honest scope)

- **VMO-backed writes** (large writes as VMO handles) — the write surface stays bounded inline
  text in v1 (RFC-0073); the read splice proves the plane both providers will share.
- **Kernel-enforced sealing / write-map denial / lifecycle closure** — RFC-0040 delegates these
  to `TASK-0290`; the splice uses the transfer floor honestly and counts fallbacks, but does not
  claim kernel-enforced seal guarantees.
