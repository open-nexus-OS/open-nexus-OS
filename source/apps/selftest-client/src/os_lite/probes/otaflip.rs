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
//!
//! TASK-0321 P3 (profile `ota-bundle`) runs the SAME two-boot shape over
//! `bundle-set.nxs` (os-B + the system volume + metricsd@1.0.1 as ONE
//! set) and adds the boot-2 volume rung: bundlemgrd verified system-b and
//! serves the set's metricsd version — the volume travelled with the flip.
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
const BUNDLE_SENTINEL_KEY: &str = "/state/app/selftest/otabundle.proof";

/// Which crown lane runs: the boot-image flip (TASK-0179) or the
/// bundle-set flip (TASK-0321 P3).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lane {
    Flip,
    Bundle,
}

impl Lane {
    fn sentinel(self) -> &'static str {
        match self {
            Lane::Flip => SENTINEL_KEY,
            Lane::Bundle => BUNDLE_SENTINEL_KEY,
        }
    }
    fn path(self) -> &'static str {
        match self {
            Lane::Flip => updated::REAL_PATH,
            Lane::Bundle => updated::BUNDLE_SET_PATH,
        }
    }
    fn staged_ok(self) -> &'static str {
        match self {
            Lane::Flip => crate::markers::M_SELFTEST_OTA_FLIP_STAGED_OK,
            Lane::Bundle => crate::markers::M_SELFTEST_OTA_BUNDLE_SET_STAGED_OK,
        }
    }
    fn stage_fail(self) -> &'static str {
        match self {
            Lane::Flip => crate::markers::M_SELFTEST_OTA_FLIP_STAGE_FAIL,
            Lane::Bundle => crate::markers::M_SELFTEST_OTA_BUNDLE_SET_STAGE_FAIL,
        }
    }
    fn ok(self) -> &'static str {
        match self {
            Lane::Flip => crate::markers::M_SELFTEST_OTA_FLIP_OK,
            Lane::Bundle => crate::markers::M_SELFTEST_OTA_BUNDLE_SET_OK,
        }
    }
    fn fail(self) -> &'static str {
        match self {
            Lane::Flip => crate::markers::M_SELFTEST_OTA_FLIP_FAIL,
            Lane::Bundle => crate::markers::M_SELFTEST_OTA_BUNDLE_SET_FAIL,
        }
    }
}
const BOOTCTL_OP_HEALTH_OK: u8 = 3;
const BOOTCTL_STATUS_OK: u8 = 0;

/// Runs the two-boot crown proof. Boot 1 ENDS IN A REBOOT (never returns);
/// boot 2 returns and the reduced ladder continues.
pub(crate) fn ota_flip_proof(statefsd: &KernelClient) {
    run_lane(statefsd, Lane::Flip)
}

/// TASK-0321 P3: the bundle-set crown proof (same two-boot shape).
pub(crate) fn ota_bundle_proof(statefsd: &KernelClient) {
    run_lane(statefsd, Lane::Bundle)
}

/// TASK-0035 P1: stage the bundle set (no switch). Boot 1 never returns —
/// the harness power-cuts the machine at the first `updated: bundle reused`
/// (after that window's journal entry is durable). Boot 2 stages again; the
/// engine resumes from the journal (`updated: restage resume (bundles=k/N)`)
/// and a successful stage is the verdict.
pub(crate) fn ota_bundle_resume_proof() {
    let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
    let Ok(updated_client) = route_with_retry("updated") else {
        emit_line(crate::markers::M_SELFTEST_OTA_STAGE_RESUME_FAIL);
        return;
    };
    let (reply_send_slot, reply_recv_slot) = reply_slots();
    let staged = updated::updated_stage_path(
        &updated_client,
        reply_send_slot,
        reply_recv_slot,
        &mut pending,
        updated::BUNDLE_SET_PATH,
    );
    emit_line(if staged.is_ok() {
        crate::markers::M_SELFTEST_OTA_STAGE_RESUME_OK
    } else {
        crate::markers::M_SELFTEST_OTA_STAGE_RESUME_FAIL
    });
}

fn run_lane(statefsd: &KernelClient, lane: Lane) {
    match sentinel_phase(statefsd, lane) {
        Some(0) => boot1_stage_and_switch(statefsd, lane),
        Some(1) => boot2_prove_flip(statefsd, lane),
        _ => emit_line(lane.fail()),
    }
}

fn boot1_stage_and_switch(statefsd: &KernelClient, lane: Lane) -> ! {
    let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
    let Ok(updated_client) = route_with_retry("updated") else {
        emit_line(lane.fail());
        park_loud()
    };
    let (reply_send_slot, reply_recv_slot) = reply_slots();

    // The real container — these are the bytes boot 2 executes.
    if updated::updated_stage_path(
        &updated_client,
        reply_send_slot,
        reply_recv_slot,
        &mut pending,
        lane.path(),
    )
    .is_err()
    {
        emit_line(lane.stage_fail());
        park_loud()
    }
    if updated::updated_switch(&updated_client, reply_send_slot, reply_recv_slot, 2, &mut pending)
        .is_err()
    {
        emit_line(lane.stage_fail());
        park_loud()
    }
    emit_line(lane.staged_ok());

    if write_sentinel(statefsd, lane, b"f1").is_err() {
        emit_line(lane.fail());
        park_loud()
    }
    reboot_now()
}

fn boot2_prove_flip(statefsd: &KernelClient, lane: Lane) {
    let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
    let Ok(updated_client) = route_with_retry("updated") else {
        emit_line(lane.fail());
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
            emit_line(lane.fail());
            let _ = del_sentinel(statefsd, lane);
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

    let _ = del_sentinel(statefsd, lane);
    // TASK-0321 P3: the bundle lane additionally proves the VOLUME travelled
    // with the flip — bundlemgrd verified system-b and serves metricsd@1.0.1
    // (the version the set carried, not the factory one).
    let bundle_ok = lane != Lane::Bundle || bundle_set_proof();
    // The VERDICT is the machine's own committed state, not the transport
    // acks of the individual health reports: `committed` is strictly
    // stronger evidence than "both reporter calls returned Ok" (a lost ack
    // on a report that still landed must not fail a flip the authority
    // has already blessed). Lost acks stay VISIBLE in the rungs line.
    if committed && bundle_ok {
        if !(selftest_ok && updated_ok) {
            emit_rungs(selftest_ok, updated_ok, committed);
        }
        emit_line(lane.ok());
    } else {
        emit_rungs(selftest_ok, updated_ok, committed);
        emit_line(lane.fail());
    }
}

/// One bundlemgrd request over the pre-distributed client pair, bounded;
/// the reply for `want_op` (foreign frames on the pair are skipped).
fn bundlemgrd_call(client: &KernelClient, req: &[u8], want_op: u8) -> Option<Vec<u8>> {
    use nexus_ipc::budget::{recv_budgeted, send_budgeted, OsClock};
    let clock = OsClock;
    send_budgeted(&clock, client, req, core::time::Duration::from_secs(2)).ok()?;
    for _ in 0..8 {
        let rsp = recv_budgeted(&clock, client, core::time::Duration::from_secs(2)).ok()?;
        if rsp.len() >= 4
            && rsp[0] == nexus_abi::bundlemgrd::MAGIC0
            && rsp[1] == nexus_abi::bundlemgrd::MAGIC1
            && rsp[3] == (want_op | 0x80)
        {
            return Some(rsp);
        }
    }
    None
}

/// Boot 2 of the bundle lane: system-b verified + metricsd@1.0.1 served
/// from it (the set's version, not the factory 1.0.0).
fn bundle_set_proof() -> bool {
    use nexus_abi::bundlemgrd as wire;
    let Ok(client) = route_with_retry("bundlemgrd") else {
        emit_line("updated-bundle: bundlemgrd unreachable");
        return false;
    };
    let mut req = [0u8; 64];
    let Some(n) = wire::encode_volume_status(&mut req) else { return false };
    let Some(rsp) = bundlemgrd_call(&client, &req[..n], wire::OP_VOLUME_STATUS) else {
        emit_line("updated-bundle: volume status unavailable");
        return false;
    };
    match wire::decode_volume_status_rsp(&rsp) {
        Some((wire::STATUS_OK, b'b', 1, _, _)) => {}
        _ => {
            emit_line("updated-bundle: system volume not verified on slot b");
            return false;
        }
    }
    let Some(n) = wire::encode_query_bundle(b"metricsd", &mut req) else { return false };
    let Some(rsp) = bundlemgrd_call(&client, &req[..n], wire::OP_QUERY_BUNDLE) else {
        emit_line("updated-bundle: bundle query unavailable");
        return false;
    };
    match wire::decode_query_bundle_rsp(&rsp) {
        Some((wire::STATUS_OK, _, _, _, _, version)) if version == b"1.0.1" => true,
        _ => {
            emit_line("updated-bundle: metricsd is not the set's 1.0.1 on slot b");
            false
        }
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

fn sentinel_phase(statefsd: &KernelClient, lane: Lane) -> Option<u8> {
    let req = proto::encode_key_only_request(proto::OP_GET, lane.sentinel()).ok()?;
    let rsp = statefs_send_recv(statefsd, &req).ok()?;
    match proto::decode_get_response(&rsp) {
        Ok(value) if value == b"f1" => Some(1),
        Ok(_) => None,
        Err(statefs::StatefsError::NotFound) => Some(0),
        Err(_) => None,
    }
}

fn write_sentinel(statefsd: &KernelClient, lane: Lane, stamp: &[u8]) -> Result<(), ()> {
    let put = proto::encode_put_request(lane.sentinel(), stamp).map_err(|_| ())?;
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

fn del_sentinel(statefsd: &KernelClient, lane: Lane) -> Result<(), ()> {
    let del = proto::encode_key_only_request(proto::OP_DEL, lane.sentinel()).map_err(|_| ())?;
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
