---
schema: 1
title: "gpud command ring and pipelined present"
role: reference
subsystem: ui
sources: [source/drivers/gpud/src/backend/virtqueue.rs, source/drivers/gpud/src/backend/virtqueue, source/drivers/gpud/src/backend/transport.rs, source/drivers/gpud/src/gl_scanout.rs, source/drivers/gpud/src/virgl.rs, source/drivers/gpud/src/markers.rs]
contracts: [ADR-0032, ADR-0033, RFC-0063]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# gpud command ring and pipelined present

> **Purpose** — how gpud submits commands to QEMU's virtio-gpu control queue and why the virgl present never blocks on its own frame.
> **Owns** — `source/drivers/gpud/src/backend/virtqueue.rs`, `source/drivers/gpud/src/backend/virtqueue/`, `source/drivers/gpud/src/backend/transport.rs`, `source/drivers/gpud/src/gl_scanout.rs`, `source/drivers/gpud/src/virgl.rs`, `source/drivers/gpud/src/markers.rs`
> **Contracts** — [ADR-0032](../adr/0032-gpu-command-ring-and-pipelined-present.md) (the decision), [ADR-0033](../adr/0033-soft-real-time-spine.md) (`nexus_driverkit::SubmitRing`), [RFC-0063](../rfcs/RFC-0063-ui-v5b-scene-graph-gpu-pipeline-virtual-list-theme-contract.md)
> **Invariants** — a slot is reused only after its completion is harvested, so QEMU never reads a buffer gpud is overwriting. Completion waits on the GPU ring-buffer IRQ beside the device watchdog, never a spin and never a receive deadline. `CtrlQueue` is `!Send`/`!Sync`.
> **Read next** — [display-chain.md](display-chain.md), [README.md](README.md)

This page covers the virtio path only (QEMU). The board's display has its own flip ring ([display-chain.md](display-chain.md#board-flips-and-cadence)).

## Why a ring

ADR-0032 records the two failures of the old one-command-at-a-time queue: a `SUBMIT_3D` that samples a texture completes only at a later `QUEUE_NOTIFY` (a blocking wait per draw cost 1–3 s per frame), and `Submit3d` allocated on gpud's non-freeing heap per draw.

## The ring (`CtrlQueue`, `backend/virtqueue.rs`)

| item | value |
|---|---|
| `RING_SLOTS` | 32 command slots (16 split busy scenes across kicks — a visible wallpaper flash) |
| `QUEUE_LEN` | `RING_SLOTS * 2` descriptors: one cmd→resp pair per slot |
| command pool | `slots` × 4 KiB, physically contiguous, made for the device |
| response pool | `slots` × `RESP_SLOT_SIZE` (256 B), rounded up to whole pages |
| slot lifecycle | `nexus_driverkit::SubmitRing` (in-flight set, round-robin, back-pressure) |
| harvest cursor | `last_used: u16` into `used.ring` |
| wait bound | `GPU_WAIT_DEADLINE_NS` = 500 ms, owned by the device watchdog (`GpuWatchdog`) |

- `RingSlot(u16)` — newtype; `head_desc() = 2*slot`, `resp_desc() = 2*slot+1`. Keeps slot, descriptor index and in-flight count apart in the unsafe arithmetic.
- `enqueue_pair` / `enqueue_single` — write the slot's buffer and chain, publish to the avail ring, notify, mark busy. No wait.
- `harvest` — walk new `used.ring` entries; `id / 2` is the finished slot.
- `alloc_free_slot` (`virtqueue/ring_wait.rs`) — harvest, take a free slot; on a full ring block on the IRQ (back-pressure).
- `wait_slot` (`virtqueue/ring_wait.rs`) — synchronous wait for one slot.

## Two submit modes

| caller | path | waits |
|---|---|---|
| init, 2D present, every non-batched command (`submit_two`, `submit_no_response`) | `enqueue_*` → `wait_slot` → `classify_resp` | yes |
| virgl present (`gl_scanout.rs`) | `ctrl_batch_begin` → every draw + flush enqueued → `ctrl_batch_end` (`backend/transport.rs`) | no |

In batch mode consecutive `SUBMIT_3D` streams are coalesced into one pending entry (`flush_pending_3d`), shipped before any non-3D command and at batch end, so the device sees the issue order. `ctrl_batch_end` harvests earlier frames but never blocks on this one: frame N+1's notifies drive frame N's completion. The flip (`SET_SCANOUT` + flush) is the batch tail, so the host shows only complete frames.

`Submit3d` (`virgl.rs`) holds its dwords inline (`[u32; 1024]`: one command is at most 4096 B) and `as_bytes()` is a zero-copy view. A draw allocates nothing.

## Hop markers (`markers.rs`)

```
gpud: chain G1 recv present-damage
gpud: chain G2 parse ok
gpud: chain G3 exec ok (commands applied)
gpud: chain G3b batch submit ok (present enqueued)     GPUD_CHAIN_BATCH_SUBMIT
gpud: chain G4 scanout ok (frame presented)
gpud: chain G3c batch complete (drained)               GPUD_CHAIN_BATCH_OK
```

`gpud: gpu irq wake` (once) proves the IRQ is the completion source. `gpud: chain G3c batch FAIL (drain timeout)` names a drain that hit the watchdog.

## Pacing outside presents

The self-presented phases (bootstrap splash, splash hold, spin demo) are paced by `source/drivers/gpud/src/frame_clock.rs`: a one-shot kernel timer on a declared timer-notify endpoint, disarmed otherwise. There is no receive timeout. windowd-driven presents are paced by windowd and the acks ([display-chain.md](display-chain.md#a-present)).

## Tests

- `tools/nx/tests/chain_gpu_scanout.rs` — `chain_gpu_batched_present_hops_in_order` pins `G3 → G3b → G3c → G4` through `GpudContract::with_batched_present()` (`tools/nx/src/chain/contract/gpud.rs`); `cargo test -p nx`.
- `cargo test -p gpud` — `Submit3d` byte-format goldens, protocol sizes.
- `just test-os visible` (virgl) — the hop ladder, `gpud: present us` stats, `gpud: gpu irq wake`.
