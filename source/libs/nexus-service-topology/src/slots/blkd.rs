// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The block owner's (`blkd`) declared slots (TASK-0324 P4f-1b) — split out of
//! `slots/mod.rs` by the structure gate when the read-only tree joined them (TASK-0246 P4b).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal — the numbers are the contract between init and the block owner

use super::SlotPair;

/// blkd's own server endpoint (block-plane clients send here). It holds NO reply
/// inbox: the driver makes no outbound call, and the inbox init used to provision was
/// never read — declaring it at 5/6 collided with the driver's own virtqueue VMOs, which it
/// allocates at the lowest free slots because it runs before init wires it.
pub const SERVER: SlotPair = crate::SERVER_SLOTS;
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
