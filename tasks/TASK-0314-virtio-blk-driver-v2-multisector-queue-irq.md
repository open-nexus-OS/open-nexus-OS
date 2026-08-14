---
title: TASK-0314 Block driver v2: multi-sector requests + real queue depth + IRQ completion (the storage perf multiplier)
status: Draft
owner: @runtime
created: 2026-08-14
depends-on: []
follow-up-tasks:
  - TASK-0315
  - TASK-0318
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Topology contract: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md
  - Store contract: docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md
  - IRQ mechanism: kernel PLIC irq_bind/irq_complete (syscalls 36/37, see docs/architecture/)
  - Ladder: tasks/TRACK-STASH-USER-DATA-FS.md
---

## Context

The virtio-blk driver is the performance multiplier under the ENTIRE storage stack (statefs
and nxfs). Measured reality (`source/drivers/storage/virtio-blk/src/lib.rs`):

- `submit()` builds a fixed 3-descriptor chain at indices 0/1/2 every time and always publishes
  descriptor 0 — effective queue depth is **1 in-flight request** despite `QUEUE_LEN = 8`.
- The data descriptor is hardcoded to **one 512 B sector** through a single shared one-page
  bounce buffer; a request can never carry more.
- Completion is a **spin-with-yield poll loop** (`nsec()` + `yield_()` per iteration) — every
  512 B of IO costs at least one scheduler round-trip. No IRQ path.
- `nxfs::Dev` (`userspace/nxfs/src/dev.rs`) therefore issues **8 serialized virtio requests per
  4 KiB logical block**, heap-allocating a fresh `vec![0u8; sector_len]` per block read.
- A long sequential read can deadlock the queue; TASK-0293 worked around it with an incremental
  journal read instead of fixing the driver.

This task fixes the driver at the root. It is deliberately **topology-neutral**: it hardens the
driver in place so both current owners (statefsd direct-MMIO, vfsd DataStore) speed up
immediately, and TASK-0315 then moves the hardened driver into virtioblkd unchanged.

## Goal

- **Multi-sector requests**: one virtio request carries a full logical block (4 KiB = 1 request,
  not 8) and, where callers pass runs, multiple contiguous blocks (bounded run length).
- **Real queue depth**: descriptor free-list, multiple in-flight requests, batched avail-ring
  publishing; `QUEUE_LEN` becomes true capacity, not decoration.
- **IRQ completion**: replace the yield-poll loop with PLIC IRQ → endpoint notification
  (irq_bind/irq_complete), with a bounded-poll fallback only where an IRQ line is unavailable
  (documented, marker-honest).
- **Deadlock fix at the root**: the long-sequential-read hazard TASK-0293 documented is fixed in
  the driver (used/avail index handling), proven by a regression test, so the incremental-read
  workaround becomes an optimization instead of a correctness crutch.
- **`nxfs::Dev` adapter**: per-block buffer reuse (no per-read `vec!`), pass multi-block runs
  through to the driver instead of looping sector-by-sector.

## Non-Goals

- Ownership/topology changes (virtioblkd promotion, GPT, cap moves) — TASK-0315.
- Scatter-gather into caller VMOs / DMA ownership (`DmaBuffer`, TASK-0284) — a follow-up once
  the block IPC plane exists (TASK-0315/0318).
- Any on-disk format change (journal bytes of statefs and nxfs stay byte-identical).
- Kernel changes beyond using the existing irq_bind/irq_complete syscalls.

## Constraints / invariants (hard requirements)

- `BlockDevice` trait semantics unchanged for callers; correctness first: every optimization is
  covered by byte-equality tests against the v1 behavior.
- Bounded everything: max run length per request, bounded in-flight count, bounded IRQ-wait with
  deadline + error signature (wait loops must self-terminate).
- No `unwrap`/`expect` on device-provided values (used-ring indices, status bytes are untrusted
  input from the device model).
- Marker honesty: `blk: irq completion on` only when the IRQ path is actually driving
  completions; poll fallback prints `blk: poll fallback (no irq)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Driver unit tests against a mock transport: multi-sector request framing, free-list reuse,
  in-flight accounting, used-ring wraparound, the sequential-read regression case.
- `nxfs::Dev` tests: byte-equality multi-block read/write vs v1 sector loop; no per-call
  allocation on the hot path (asserted via test hook or allocation counter).

### Proof (OS / QEMU) — required

- Existing persistence/`/data` marker ladder stays green (statefs persist, `nxfsd: mounted
  /data`, stash write path).
- `blk: irq completion on` (or the honest poll-fallback marker).
- Before/after measurement recorded in the ledger: virtio requests per 4 KiB block (8 → 1) and
  per stash file write, from driver counters (deterministic, not wall-clock).

## Touched paths (allowlist)

- `source/drivers/storage/virtio-blk/`
- `userspace/nxfs/src/dev.rs`
- `source/services/statefsd/` (only if the backend adapter needs the run API)
- `source/apps/selftest-client/`, `scripts/qemu-test.sh` (new markers)
- `docs/storage/` (driver notes), `docs/testing/README.md`
