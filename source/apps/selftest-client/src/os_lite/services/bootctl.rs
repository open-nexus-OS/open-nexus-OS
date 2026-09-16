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

use statefs::protocol as statefs_proto;

use super::super::ipc::routing::route_with_retry;
use crate::markers::emit_line;

pub(crate) fn bootctl_persist_check() -> core::result::Result<(), ()> {
    const BOOTCTL_KEY: &str = "/state/boot/bootctl.v1";
    const BOOTCTL_VERSION: u8 = 1;
    emit_line(crate::markers::M_SELFTEST_BOOTCTL_PERSIST_BEGIN);
    let client = route_with_retry("statefsd")?;
    let get = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, BOOTCTL_KEY)
        .map_err(|_| ())?;
    // ONE exchange through the harness' ONE statefs helper (TASK-0054C P2-f). This probe used
    // to hand-roll the v2 nonce upgrade and read statefsd's shared response endpoint cap-less —
    // a second copy of `statefs_send_recv`, and one of the three clients that kept statefsd
    // answering cap-less senders at all.
    let rsp = super::statefs::statefs_send_recv(&client, &get)?;
    let bytes = statefs_proto::decode_get_response(&rsp).map_err(|_| ())?;
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
