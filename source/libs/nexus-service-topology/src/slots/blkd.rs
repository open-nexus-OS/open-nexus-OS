// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The block owner's (`blkd`) declared slots (TASK-0324 P4f-1b) — split out of
//! `slots/mod.rs` by the structure gate when the read-only tree joined them (TASK-0246 P4b).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal — the numbers are the contract between init and the block owner

use super::SlotPair;

/// blkd's own server endpoint (block-plane clients send here).
pub const SERVER: SlotPair = crate::SERVER_SLOTS;
/// The CAP_MOVE reply inbox of its one outbound call — asking socd to bring its node up
/// (TASK-0246 P4c). High slots on purpose: the virtio driver allocates its virtqueue VMOs at
/// the lowest free slots, which is where an inbox at 5/6 once collided with them; init pins
/// this one before blkd runs.
pub const REPLY: SlotPair = SlotPair::new(0xF8, 0xF7);
/// The route to socd (RFC-0106): `BRING_UP` of the disk's node before the controller is
/// touched, and on the K1 the `io` clock's rate.
pub const SOCD: SlotPair = SlotPair::new(0xF9, REPLY.recv);
/// RECV half of the dedicated IRQ-completion notify endpoint. It used to sit at 0xF1 —
/// the same number as every block-plane client's reply RECV, so a reader could not tell
/// the driver's IRQ slot from a client's reply slot; 0xF3 extends the block-plane family
/// instead of aliasing into it (both sides read this constant, so the move is safe by
/// construction).
pub const IRQ_NOTIFY: u32 = 0xF3;
/// Device-watchdog endpoint (TASK-0054C P2-b): the bound of the backend's waits — a one-shot
/// on a waitset beside the IRQ endpoint (virtio-blk: its fire is `virtio-blk: timeout`; the
/// SDHCI core, TASK-0246 P4b: every bounded wait and delay).
pub const WATCHDOG: SlotPair = SlotPair::new(0xF5, 0xF4);
/// The read-only device tree (TASK-0246 P4b): the owner reads the loader's boot-disk
/// record from it and checks the grant against it, and its SD host's node configures the
/// host.
pub const DEVICE_TREE: u32 = 0xF6;
/// The kernel console ring, read-only (RFC-0107 Phase 2): the owner keeps it in the boot
/// trace's slot for this boot (`/chosen/nexus,trace`).
pub const CONSOLE_RING: u32 = 0xFA;
/// The trace's pacing timer (RFC-0107 Phase 2, RFC-0093 §7): a one-shot armed while the ring
/// holds bytes the trace does not, a waitset member beside the server endpoint.
pub const TRACE_TIMER: SlotPair = SlotPair::new(0xFC, 0xFB);
