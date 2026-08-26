// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: nxfsd's block-plane attach (TASK-0315): init wires the
//! virtioblkd request SEND + a dedicated reply pair at FIXED slots
//! (blockproto SSOT 0xF0..0xF2) during spawn-time distribution — present
//! BEFORE any request can flow, so the pristine upgrade window keeps the
//! same ordering guarantee the old direct-MMIO grant had. The attach gate
//! is a LOCAL cap presence check (never a route round-trip: a slow
//! resolver must not consume the window nor stall requests).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (`nxfsd: virtio upgrade ok` over IPC +
//!   keep-blk double boot).
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use storage::blockproto::{
    CLIENT_REPLY_RECV_SLOT, CLIENT_REPLY_SEND_SLOT, CLIENT_REQ_SLOT, PART_DATA,
};
use storage::remote_blk::RemoteBlockDevice;

/// One bounded attach attempt for the DATA partition. The cap-presence
/// gate is free; the INFO round trip is bounded (1 s) so early attempts
/// while virtioblkd still initializes its device stay cheap for the
/// upgrade window's retry budget.
pub(crate) fn attach_data_partition() -> Option<RemoteBlockDevice> {
    RemoteBlockDevice::open_with_deadline(
        CLIENT_REQ_SLOT,
        CLIENT_REPLY_SEND_SLOT,
        CLIENT_REPLY_RECV_SLOT,
        PART_DATA,
        1_000_000_000,
    )
}
