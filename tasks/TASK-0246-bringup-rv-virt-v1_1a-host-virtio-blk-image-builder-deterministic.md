---
title: TASK-0246 RISC-V Bring-up v1.1a (host-first): virtio-blk frontend core + packagefs image builder + deterministic tests
status: Draft
owner: @kernel
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Persistence baseline (virtio-blk): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Device MMIO access: tasks/TASK-0010-device-mmio-access-model.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need virtio-blk support for packagefs mounting:

- virtio-blk frontend (read-only) for userspace,
- packagefs image builder (deterministic),
- deterministic host tests.

The prompt proposes userspace virtio-blk (read-only) and a packagefs image builder. `TASK-0009` already plans virtio-blk for persistence (read-write). This task delivers the **host-first core** (virtio-blk frontend library, image builder) that can be reused by both packagefs (read-only) and statefs (read-write).

## Goal

Deliver on host:

1. **Virtio-blk frontend library** (`userspace/libs/virtio-blk/` or `source/drivers/storage/virtio-blk/`):
   - virtio-mmio blk frontend (legacy virtio 0.9/modern common subset)
   - read-only feature bit negotiation
   - queue setup (q=0): allocate descriptor/ring buffers
   - `read(lba, count)` → bounded to 128 KiB per call
   - `info()` → returns sector count/size
   - deterministic ring math (wrap-around handling)
2. **Packagefs image builder** (`tools/mk-pkgfs-img/`):
   - builds raw image from `pkg://fixtures/**` (existing packagefs files)
   - simple block layout expected by packagefs
   - deterministic order, fixed mtimes/ownership, sector-aligned
   - produces `build/pkgfs.img` used by runner
3. **Host tests** proving:
   - virtio ring math: descriptor/avail/used ring wrap-around with fake MMIO backend
   - packagefs image reader: build tiny image via `mk-pkgfs-img` with two files; read via vblk shim → hashes match

## Non-Goals

- OS/QEMU integration (deferred to v1.1b).
- Write support (read-only for packagefs only).
- Full virtio spec compliance (minimal subset only).

## Constraints / invariants (hard requirements)

- **No duplicate virtio-blk authority**: This task provides a reusable virtio-blk frontend library. `TASK-0009` will use it for statefs (read-write), while this task focuses on packagefs (read-only).
- **Determinism**: virtio ring math, image building, and reading must be stable given the same inputs.
- **Bounded resources**: image building is size-bounded; read operations are bounded (128 KiB per call).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **YELLOW (virtio-blk vs TASK-0009)**:
  - `TASK-0009` already plans virtio-blk for statefs. This task should provide a reusable library that both packagefs and statefs can use. Document the relationship explicitly.

## Contract sources (single source of truth)

- Testing contract: `scripts/qemu-test.sh`
- Persistence baseline: `TASK-0009` (virtio-blk for statefs)
- Device MMIO access: `TASK-0010` (prerequisite for userspace virtio)

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p bringup_rv_virt_v1_1_host` green (new):

- virtio ring math: descriptor/avail/used ring wrap-around with fake MMIO backend
- packagefs image reader: build tiny image via `mk-pkgfs-img` with two files; read via vblk shim → hashes match

## Touched paths (allowlist)

- `userspace/libs/virtio-blk/` (new; or extend `source/drivers/storage/virtio-blk/`)
- `tools/mk-pkgfs-img/` (new)
- `tests/bringup_rv_virt_v1_1_host/` (new)
- `docs/storage/virtio_blk.md` (new, host-first sections)

## Plan (small PRs)

1. **Virtio-blk frontend library**
   - virtio-mmio blk frontend (read-only)
   - ring math (descriptor/avail/used)
   - host tests

2. **Packagefs image builder**
   - image builder tool
   - deterministic layout
   - host tests

3. **Docs**
   - host-first docs

## Acceptance criteria (behavioral)

- Virtio-blk frontend library handles ring math correctly.
- Packagefs image builder produces deterministic images.
- Host tests pass.
