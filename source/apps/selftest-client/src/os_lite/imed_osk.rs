// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: imed OSK-endpoint probe (RFC-0075 Phase 2): the selftest holds
//! an init-provisioned SEND on imed's DEDICATED osk endpoint (possession =
//! authorization) and proves BOTH directions: a well-formed `source=osk`
//! key is ACCEPTED (STATUS_OK on the probe's own reply channel), a
//! mis-tagged `source=hw` frame on the same endpoint is DENIED. The
//! commit-at-focused-field chain is the interactive OSK proof (`just
//! start`); this lane proves authorization + codec + serve loop. Fixture
//! character only — no real text.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os`.
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

use core::time::Duration;

use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};

use super::ipc::routing::route_with_retry;

fn mint_pair() -> Option<(u32, u32)> {
    match budget::route_with_nonce_budgeted(
        b"@mint-pair",
        1,
        2,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => Some((send_slot, recv_slot)),
        _ => None,
    }
}

/// Sends one OP_KEY frame on the osk endpoint with a CAP_MOVE'd reply SEND
/// and returns the reply status byte (deadline-blocked recv, never a spin).
fn osk_key_status(osk_send: u32, source: u8, ch: char) -> Result<u8, ()> {
    let (ev_send, ev_recv) = mint_pair().ok_or(())?;
    let mut req = [0u8; 12];
    req[0] = b'I';
    req[1] = b'E';
    req[2] = 1; // VERSION
    req[3] = 2; // OP_KEY
    req[4] = source;
    req[5] = 0; // KEY_KIND_TEXT
    req[6..10].copy_from_slice(&u32::from(ch).to_le_bytes());
    req[10] = 0; // action
    req[11] = 0; // modifiers
    let hdr = nexus_abi::MsgHeader::new(ev_send, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, 12);
    if nexus_abi::ipc_send_v1(osk_send, &hdr, &req, nexus_abi::IPC_SYS_NONBLOCK, 0).is_err() {
        return Err(());
    }
    let deadline = nexus_abi::nsec().map_err(|_| ())?.saturating_add(800_000_000);
    let mut rhdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut sid: u64 = 0;
    let mut buf = [0u8; 64];
    let len = nexus_abi::ipc_recv_v2(
        ev_recv,
        &mut rhdr,
        &mut buf,
        &mut sid,
        nexus_abi::IPC_SYS_TRUNCATE,
        deadline,
    )
    .map_err(|_| ())? as usize;
    // Reply frame: [I, E, 1, OP_KEY|0x80, status].
    if len == 5 && buf[0] == b'I' && buf[1] == b'E' && buf[3] == (2 | 0x80) {
        Ok(buf[4])
    } else {
        Err(())
    }
}

pub(crate) fn imed_osk_probe() -> Result<(), ()> {
    // The osk route is init-provisioned for the harness; resolving it also
    // proves the imed-osk named route exists.
    let client = route_with_retry("imed-osk").map_err(|_| ())?;
    let (osk_send, _) = client.slots();
    // Positive: a well-formed osk-sourced key is ACCEPTED (STATUS_OK = 0).
    if osk_key_status(osk_send, 1, 'x')? != 0 {
        return Err(());
    }
    // Negative: a hw-tagged frame on the OSK endpoint is DENIED (2).
    if osk_key_status(osk_send, 0, 'x')? != 2 {
        return Err(());
    }
    Ok(())
}
