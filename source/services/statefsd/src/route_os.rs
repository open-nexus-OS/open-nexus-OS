// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: statefsd's block-plane attach (TASK-0315): init wires the
//! virtioblkd request SEND + a dedicated reply pair at FIXED slots
//! (blockproto SSOT 0xF0..0xF2) during spawn-time distribution — present
//! BEFORE any request can flow, so the pristine upgrade window keeps the
//! same ordering guarantee the old direct-MMIO grant had. The attach gate
//! is a LOCAL cap presence check (never a route round-trip: a slow
//! resolver must not consume the window nor stall requests).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (`statefsd: virtio upgrade ok` over IPC +
//!   keep-blk double boot).
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use storage::blockproto::{
    CLIENT_REPLY_RECV_SLOT, CLIENT_REPLY_SEND_SLOT, CLIENT_REQ_SLOT, PART_STATE,
};
use storage::remote_blk::RemoteBlockDevice;

/// One attach attempt for the STATE partition. BLOCKS bounded (8 s) once
/// the wiring is present — mirroring the old direct-MMIO upgrade, where
/// the inline device init also ran to completion on the first request:
/// the pristine window must never lose to virtioblkd still bringing the
/// device up. A `None` after the bound is a REAL failure the window's
/// bounded retry budget accounts.
pub(crate) fn attach_state_partition() -> Option<RemoteBlockDevice> {
    RemoteBlockDevice::open_with_deadline(
        CLIENT_REQ_SLOT,
        CLIENT_REPLY_SEND_SLOT,
        CLIENT_REPLY_RECV_SLOT,
        PART_STATE,
        8_000_000_000,
    )
}
