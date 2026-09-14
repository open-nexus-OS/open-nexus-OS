// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: bootctl IPC client used by the OTA phase — slot status / mark-good
//!   roundtrips that anchor the A/B switch and rollback proofs.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — ota phase.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::vec::Vec;

use nexus_abi::MsgHeader;
use statefs::protocol as statefs_proto;

use super::super::ipc::routing::route_with_retry;
use crate::markers::emit_line;

pub(crate) fn bootctl_persist_check() -> core::result::Result<(), ()> {
    const BOOTCTL_KEY: &str = "/state/boot/bootctl.v1";
    const BOOTCTL_VERSION: u8 = 1;
    emit_line(crate::markers::M_SELFTEST_BOOTCTL_PERSIST_BEGIN);
    let client = route_with_retry("statefsd")?;
    let (send_slot, recv_slot) = client.slots();
    // Deterministic: use SF v2 (nonce) and only accept the matching reply.
    static NONCE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let get_v1 = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, BOOTCTL_KEY)
        .map_err(|_| ())?;
    // Upgrade v1 request frame to v2 by inserting nonce after the 4-byte header.
    let mut get = Vec::with_capacity(get_v1.len().saturating_add(8));
    get.extend_from_slice(&get_v1[..4]);
    get[2] = statefs_proto::VERSION_V2;
    get.extend_from_slice(&nonce.to_le_bytes());
    get.extend_from_slice(&get_v1[4..]);
    // A waited send, then a waited receive (statefsd's answer or its death) — no clock.
    let hdr = MsgHeader::new(0, 0, 0, 0, get.len() as u32);
    if nexus_abi::ipc_send_v1(send_slot, &hdr, &get, 0, 0).is_err() {
        return Err(());
    }
    // Recv.
    let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 512];
    let n = loop {
        match nexus_abi::ipc_recv_v1(recv_slot, &mut rh, &mut buf, nexus_abi::IPC_SYS_TRUNCATE, 0) {
            Ok(n) => break n as usize,
            Err(_) => return Err(()),
        }
    };
    let n = core::cmp::min(n, buf.len());
    if n < 13 || buf[0] != statefs_proto::MAGIC0 || buf[1] != statefs_proto::MAGIC1 {
        return Err(());
    }
    if buf[2] != statefs_proto::VERSION_V2 {
        return Err(());
    }
    let got_nonce =
        u64::from_le_bytes([buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12]]);
    if got_nonce != nonce {
        return Err(());
    }
    let bytes = statefs_proto::decode_get_response(&buf[..n]).map_err(|_| ())?;
    // TASK-0025 step 4: updated seals the record as an Integrity envelope;
    // unwrap it (legacy raw bytes from pre-migration journals pass through).
    let stored = statefs::writer::open_stored(&bytes).map_err(|_| ())?;
    let payload = stored.payload();
    // Known record shapes: v1 (updated-era, 6 bytes), v2 (bootctld
    // authority, 9 bytes with the rollback + target axis — TASK-0050),
    // v3 (22 bytes, RFC-0089 §13 quorum/deadline/floor — TASK-0036-A) and
    // v4 (26 bytes, + staged rollback index — TASK-0179). Anything else is
    // corrupt. NOTE: every record version bump must land here too; the
    // probe deliberately pins EXACT shapes, because a persisted record of
    // an unknown length is the one thing this proof exists to catch.
    let valid = matches!(
        (payload.first().copied(), payload.len()),
        (Some(BOOTCTL_VERSION), 6) | (Some(2), 9) | (Some(3), 22) | (Some(4), 26)
    );
    if !valid {
        return Err(());
    }
    Ok(())
}
