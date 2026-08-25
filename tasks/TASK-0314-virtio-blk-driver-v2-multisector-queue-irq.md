---
title: TASK-0314 Block driver v2: multi-sector requests + real queue depth + IRQ completion (the storage perf multiplier)
status: Done
completed: 2026-08-25
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

## DELIVERED 2026-08-25 (test-all green; OTA-lane package 3)

Evidence (`just test-all` EXIT=0 incl. headless/reset/SMP; headless uart
2026-08-25 `blk: poll fallback (no irq)` gated):

- **Multi-sector runs**: `read_run`/`write_run` carry up to `MAX_RUN_BYTES`
  (16 KiB = 32 sectors) in ONE request; `BlockDevice` gained default
  `read_blocks`/`write_blocks` run methods, overridden by
  `VirtioBlkDevice` with run-chunked driver calls.
- **Real queue depth**: `QUEUE_LEN` 8 → 64 with the new host-proven request
  ring (`src/ring.rs`): descriptor free-list, chained allocation with
  rollback on partial reservation, per-id in-flight accounting, used-ring
  reclamation that validates DEVICE-PROVIDED ids (out-of-range /
  not-in-flight → loud error, never an index panic). `IN_FLIGHT_MAX` = 4
  bounded request slots (own header/status/data regions).
- **Deadlock fix at the root**: the v1 code published descriptor 0 for
  every request and compared only a used-idx counter (the TASK-0293
  long-sequential-read hazard). Regression proof:
  `test_used_ring_wraparound_and_long_sequential_regression` drives 70 000
  sequential requests across the u16 index wrap; out-of-order completion
  and backpressure-rollback tests cover the rest. 8 ring host tests total.
- **`nxfs::Dev` adapter**: one RUN request per 4 KiB logical block (was 8
  serialized sector requests + a per-read heap `vec!`). Deterministic
  measurement (ledger DoD): `logical_block_uses_one_run_request` asserts
  run_calls == 1 and sector_calls == 0 per logical block, byte-identical
  to the sector-loop path — the 8 → 1 count is proven by op counters, not
  wall clock. Driver-side `requests_submitted()` counter added.
- **IRQ completion — Option-C recut (documented)**: the driver carries the
  full IRQ machinery (PLIC line derived from the granted transport window
  — index+1, the hidrawd mapping; blocking `ipc_recv_v1` wait with
  deadline; drain→InterruptStatus-ack→`irq_complete` ordering per the
  virtio-input/gpud lesson; `bind_irq_endpoint()` owner API). Endpoint
  PROVISIONING recuts to TASK-0315: `ipc_endpoint_create` v1 is
  init-factory-gated (RFC-0005 hardening), and aliasing statefsd/vfsd's
  ctrl-reply slot 2 (the hidrawd/gpud pattern) would be an interim hack
  the 0315 block server replaces one package later — virtioblkd binds a
  properly wired notify endpoint instead. Until then the HONEST marker
  `blk: poll fallback (no irq)` is gated in headless+smp1; the marker
  recuts to `blk: irq completion on` with 0315.
- Structure ratchet: the MMIO backend split out of the 850-line lib.rs
  into `src/mmio.rs` (578 LOC); lib.rs 853 → 318.

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
