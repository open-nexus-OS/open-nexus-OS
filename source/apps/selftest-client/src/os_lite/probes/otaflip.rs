// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: The OTA crown proof (TASK-0179; profile `ota-flip`) — the
//! first update that actually changes what the machine boots. Two boots,
//! ONE uart stream, sentinel-phased exactly like the reset lane:
//!
//!   Boot 1 (no sentinel): stage the REAL `os-B.nxs` into the INACTIVE
//!   slot over the apply engine (stream → digest → readback → NXBD-last),
//!   `OP_SWITCH(tries=2)` so bootctld projects `next=b tries=2` into the
//!   BSB, stamp the sentinel, SBI reset.
//!
//!   Boot 2 (sentinel `f1`): the LOADER chose slot b from that projection
//!   and printed a DIFFERENT build id — the flip already happened before
//!   any OS code ran. This probe proves the userspace half: we are running
//!   on slot b, the health quorum completes, bootctld commits the slot and
//!   raises the anti-downgrade floor to the staged index.
//!
//! Every rung fails LOUD; the lane never silently degrades into "booted
//! something".
//! OWNERS: @runtime @security
//! STATUS: Experimental (TASK-0179)
//! TEST_COVERAGE: QEMU `ota-flip` profile (two boots, one uart)
//! ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use nexus_ipc::KernelClient;
use statefs::protocol as proto;

use crate::markers::emit_line;
use crate::os_lite::ipc::routing::route_with_retry;
use crate::os_lite::updated;

use crate::os_lite::services::statefs::statefs_send_recv;

use super::reset::{bootctl_call_raw, reboot_now};

const SENTINEL_KEY: &str = "/state/app/selftest/otaflip.proof";
const BOOTCTL_OP_HEALTH_OK: u8 = 3;
const BOOTCTL_STATUS_OK: u8 = 0;

/// Runs the two-boot crown proof. Boot 1 ENDS IN A REBOOT (never returns);
/// boot 2 returns and the reduced ladder continues.
pub(crate) fn ota_flip_proof(statefsd: &KernelClient) {
    match sentinel_phase(statefsd) {
        Some(0) => boot1_stage_and_switch(statefsd),
        Some(1) => boot2_prove_flip(statefsd),
        _ => emit_line(crate::markers::M_SELFTEST_OTA_FLIP_FAIL),
    }
}

fn boot1_stage_and_switch(statefsd: &KernelClient) -> ! {
    let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
    let Ok(updated_client) = route_with_retry("updated") else {
        emit_line(crate::markers::M_SELFTEST_OTA_FLIP_FAIL);
        park_loud()
    };
    let (reply_send_slot, reply_recv_slot) = reply_slots();

    // The real container — these are the bytes boot 2 executes.
    if updated::updated_stage_real(&updated_client, reply_send_slot, reply_recv_slot, &mut pending)
        .is_err()
    {
        emit_line(crate::markers::M_SELFTEST_OTA_FLIP_STAGE_FAIL);
        park_loud()
    }
    if updated::updated_switch(&updated_client, reply_send_slot, reply_recv_slot, 2, &mut pending)
        .is_err()
    {
        emit_line(crate::markers::M_SELFTEST_OTA_FLIP_STAGE_FAIL);
        park_loud()
    }
    emit_line(crate::markers::M_SELFTEST_OTA_FLIP_STAGED_OK);

    if write_sentinel(statefsd, b"f1").is_err() {
        emit_line(crate::markers::M_SELFTEST_OTA_FLIP_FAIL);
        park_loud()
    }
    reboot_now()
}

fn boot2_prove_flip(statefsd: &KernelClient) {
    let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
    let Ok(updated_client) = route_with_retry("updated") else {
        emit_line(crate::markers::M_SELFTEST_OTA_FLIP_FAIL);
        return;
    };
    let (reply_send_slot, reply_recv_slot) = reply_slots();

    // We must be RUNNING on the slot the loader was told to try.
    // The PROOF is "the machine is running slot b". Do NOT also demand a
    // still-pending trial: init's boot-attempt handshake may already have
    // consumed it by the time this probe runs, and that ordering is not
    // what the lane is testing.
    // `updated` may still be initializing this early in bringup — poll the
    // status call to a deadline instead of treating the first transport
    // failure as a verdict.
    let route_deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(15_000_000_000);
    let mut status = Err(());
    while nexus_abi::nsec().unwrap_or(u64::MAX) < route_deadline {
        status = updated::updated_get_status(
            &updated_client,
            reply_send_slot,
            reply_recv_slot,
            &mut pending,
        );
        if status.is_ok() {
            break;
        }
        let _ = nexus_abi::yield_();
    }
    match status {
        Ok((updated::SlotId::B, _pending, _tries, _health)) => {}
        other => {
            emit_line(if other.is_ok() {
                "updated-flip: wrong slot after reset (expected b)"
            } else {
                "updated-flip: status unavailable after reset"
            });
            emit_line(crate::markers::M_SELFTEST_OTA_FLIP_FAIL);
            let _ = del_sentinel(statefsd);
            return;
        }
    }

    // Health quorum v2: both declared reporters confirm, bootctld commits
    // the slot and raises the floor (RFC-0089 §10 commit rung).
    let selftest_ok = matches!(
        bootctl_call_raw(&[b'B', b'T', 1, BOOTCTL_OP_HEALTH_OK], BOOTCTL_OP_HEALTH_OK),
        Some((BOOTCTL_STATUS_OK, _))
    );
    let updated_ok = updated::init_health_ok().is_ok();

    // The commit is ASYNCHRONOUS: the second reporter travels
    // init -> updated -> bootctld, so a single status read here loses the
    // race and reports a failure the machine is about to disprove. Poll to
    // a DEADLINE (wait-loop doctrine: bounded, and a timeout is an honest
    // FAIL rather than a hang).
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(15_000_000_000);
    let mut committed = false;
    while nexus_abi::nsec().unwrap_or(u64::MAX) < deadline {
        if matches!(
            updated::updated_get_status(
                &updated_client,
                reply_send_slot,
                reply_recv_slot,
                &mut pending,
            ),
            Ok((updated::SlotId::B, None, _tries, true))
        ) {
            committed = true;
            break;
        }
        let _ = nexus_abi::yield_();
    }

    let _ = del_sentinel(statefsd);
    // The VERDICT is the machine's own committed state, not the transport
    // acks of the individual health reports: `committed` is strictly
    // stronger evidence than "both reporter calls returned Ok" (a lost ack
    // on a report that still landed must not fail a flip the authority
    // has already blessed). Lost acks stay VISIBLE in the rungs line.
    if committed {
        if !(selftest_ok && updated_ok) {
            emit_rungs(selftest_ok, updated_ok, committed);
        }
        emit_line(crate::markers::M_SELFTEST_OTA_FLIP_OK);
    } else {
        emit_rungs(selftest_ok, updated_ok, committed);
        emit_line(crate::markers::M_SELFTEST_OTA_FLIP_FAIL);
    }
}

/// Names WHICH rung of the flip did not hold: an undifferentiated verdict
/// on a three-part condition sends the reader guessing (it already did).
fn emit_rungs(selftest_ok: bool, updated_ok: bool, committed: bool) {
    let mut line = [0u8; 64];
    let head = b"updated-flip: rungs selftest=0 updated=0 committed=0";
    line[..head.len()].copy_from_slice(head);
    line[29] = b'0' + u8::from(selftest_ok);
    line[39] = b'0' + u8::from(updated_ok);
    line[51] = b'0' + u8::from(committed);
    if let Ok(msg) = core::str::from_utf8(&line[..head.len()]) {
        emit_line(msg);
    }
}

/// The selftest client's own CAP_MOVE reply inbox (slot_map SSOT).
fn reply_slots() -> (u32, u32) {
    (0x18, 0x17)
}

fn sentinel_phase(statefsd: &KernelClient) -> Option<u8> {
    let req = proto::encode_key_only_request(proto::OP_GET, SENTINEL_KEY).ok()?;
    let rsp = statefs_send_recv(statefsd, &req).ok()?;
    match proto::decode_get_response(&rsp) {
        Ok(value) if value == b"f1" => Some(1),
        Ok(_) => None,
        Err(statefs::StatefsError::NotFound) => Some(0),
        Err(_) => None,
    }
}

fn write_sentinel(statefsd: &KernelClient, stamp: &[u8]) -> Result<(), ()> {
    let put = proto::encode_put_request(SENTINEL_KEY, stamp).map_err(|_| ())?;
    let rsp = statefs_send_recv(statefsd, &put)?;
    if proto::decode_status_response(proto::OP_PUT, &rsp) != Ok(proto::STATUS_OK) {
        return Err(());
    }
    let rsp = statefs_send_recv(statefsd, &proto::encode_sync_request())?;
    if proto::decode_status_response(proto::OP_SYNC, &rsp) != Ok(proto::STATUS_OK) {
        return Err(());
    }
    Ok(())
}

fn del_sentinel(statefsd: &KernelClient) -> Result<(), ()> {
    let del = proto::encode_key_only_request(proto::OP_DEL, SENTINEL_KEY).map_err(|_| ())?;
    let rsp = statefs_send_recv(statefsd, &del)?;
    let _ = proto::decode_status_response(proto::OP_DEL, &rsp);
    Ok(())
}

/// Boot 1 must never fall through into the normal ladder with a
/// half-armed flip: park visibly (the harness times out with the FAIL
/// marker already on the wire — wait-loop doctrine).
fn park_loud() -> ! {
    loop {
        let _ = nexus_abi::yield_();
    }
}
