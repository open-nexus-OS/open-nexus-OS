---
title: TASK-0033 packagefs v2b: zero-copy VMO splice from RO package image (gated, fallback-safe)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Depends-on (package image v2): tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md
  - Depends-on (VMO plumbing): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - Depends-on (ABI filter policies): tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Package image v2 provides fast lookup and read paths, but large payload reads still copy bytes through IPC.
The architecture vision expects VMO/filebuffer for bulk data-plane transfers.

This task adds a **splice-to-VMO** path while keeping a safe fallback to the existing copy-based `read`.

## Goal

Provide a capability-gated, bounded “splice to VMO” API for packagefs:

- for large reads, clients can request a VMO-backed view of an image range,
- integrity is preserved (hash verified against index),
- fallback to copy-based read remains available and tested.

## Non-Goals

- Writable packagefs.
- Cross-device transport of VMOs (DSoftBus VMO frames are separate).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- No fake success markers (only after VMO mapped and digest verified).
- Bounded budgets:
  - max splice length per request,
  - max total live VMOs served by packagefsd,
  - LRU eviction.
- Policy-gated (ABI filters + policyd), deny-by-default.

## Red flags / decision points

- **RED (VMO transfer feasibility)**:
  - If VMO handles cannot be safely transferred across processes with existing syscalls/caps, this task cannot deliver a real cross-process splice.
  - In that case, keep splice as an in-process optimization only and document it; do not claim “zero-copy”.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- For a large file entry:
  - `splice_to_vmo` returns a VMO descriptor/handle,
  - consumer maps RO and computes sha256,
  - sha256 equals index hash.
- Negative:
  - request beyond bounds fails deterministically,
  - hash mismatch fails deterministically.

### Proof (OS / QEMU) — gated

Once VMO sharing is proven:

- `packagefsd: splice→vmo ok (len=<n>)`
- `SELFTEST: pkgimg vmo ok`

## Touched paths (allowlist)

- `source/services/packagefsd/` (splice handler + budgets; gated)
- `userspace/memory/nexus-vmo/` (consumer mapping helpers)
- `source/apps/selftest-client/` (gated markers)
- `docs/storage/packagefs.md` (splice semantics + budgets)
- `scripts/qemu-test.sh`

