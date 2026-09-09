// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE readiness point of an OS service (RFC-0093 §2, ADR-0062): print the
//! `<svc>: ready` marker and announce `@ready` to init on the control channel. Split out of
//! `lib.rs` (module-size ratchet).
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P2)
//! API_STABILITY: Stable (`nexus_service_entry::ready`)
//! TEST_COVERAGE: QEMU ladders (`init: up <svc>` follows `<svc>: ready`), scripts/check-init-sync.sh

use super::{debug_write_byte, debug_write_bytes, debug_write_str, service_name};

/// The ONE readiness point of a service (RFC-0093 §2, ADR-0062): prints the service's
/// `<svc>: ready` marker through the folding-aware `debug_println` AND announces `@ready`
/// to init on the control channel, so `init: up <svc>` is init's observation of THIS call
/// and nothing else. The announce is one-way (no reply frame may land in the child's
/// control RSP queue — a stale frame there is exactly the confused-waiter class routing v2
/// removes). A failed announce is loud: init would never print `init: up`, the ladder
/// fails, and the raw witness names the service.
pub fn ready(marker: &str) -> nexus_abi::SysResult<()> {
    let printed = nexus_abi::debug_println(marker);
    announce_ready();
    printed
}

/// `@ready` on the control REQ slot (init-lite transfers it into slot 1 of every child,
/// the same slot `@reply`/`@mint-pair` queries use). Routing-frame encoding, no nonce: init
/// records it and sends nothing back.
fn announce_ready() {
    const CTRL_SEND_SLOT: u32 = 1;
    let mut buf = [0u8; 16];
    let Some(n) = nexus_abi::routing::encode_route_get(b"@ready", &mut buf) else {
        return;
    };
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, n as u32);
    if nexus_abi::ipc_send_v1(CTRL_SEND_SLOT, &hdr, &buf[..n], nexus_abi::IPC_SYS_NONBLOCK, 0)
        .is_err()
    {
        debug_write_bytes(b"FAIL ready announce svc=");
        debug_write_str(service_name());
        debug_write_byte(b'\n');
    }
}
