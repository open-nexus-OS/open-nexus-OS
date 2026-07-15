---
title: TASK-0295 Zero-copy storage data plane: VMO splice reads (packagefs + nxfs) + VMO-backed writes + inline cap enforcement
status: Draft
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
