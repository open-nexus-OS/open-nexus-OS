// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: The loader tries-exhaustion backstop (TASK-0289-B; profile
//! `ota-fallback`) — the recovery that still works when a staged image is
//! too broken to run its own userspace. Four boots, ONE uart stream,
//! sentinel-phased like the flip lane:
//!
//!   Boot 1 (no sentinel): stage the REAL `os-B.nxs` + `OP_SWITCH(2)`,
//!   stamp the sentinel, SBI reset — same arming as the crown flip.
//!
//!   Boots 2/3 (trial, slot b): init PARKS after `init: health withheld
//!   (fault fixture)` (see nexus-init `fault_fixture.rs`) — no services,
//!   no boot-attempt tick, exactly a bricked image. The harness plays the
//!   power cycle. The loader alone decrements `2->1`, `1->0`.
//!
//!   Boot 4 (sentinel present, slot a): the loader read tries=0 and
//!   flipped (`nxboot: fallback (slot=b exhausted) -> slot=a`); bootctld
//!   OBSERVED the exhaustion at attach and rolled the record back. This
//!   probe verifies the observation (active a, nothing pending) and the
//!   measured record (booted slot a), then emits the lane verdict.
//! OWNERS: @runtime @security
//! STATUS: Experimental (TASK-0289-B)
//! TEST_COVERAGE: QEMU `ota-fallback` profile (four boots, one uart)
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use nexus_ipc::KernelClient;
use statefs::protocol as proto;

use crate::markers::emit_line;
use crate::os_lite::ipc::routing::route_with_retry;
use crate::os_lite::updated;

use crate::os_lite::services::statefs::statefs_send_recv;

use super::reset::{bootctl_call_payload, reboot_now};

const SENTINEL_KEY: &str = "/state/app/selftest/otafallback.proof";

/// Runs the four-boot backstop proof. Boot 1 ENDS IN A REBOOT (never
/// returns); the final boot returns and the reduced ladder continues.
pub(crate) fn ota_fallback_proof(statefsd: &KernelClient) {
    match sentinel_phase(statefsd) {
        Some(0) => boot1_stage_and_switch(statefsd),
        Some(1) => final_boot_verify(statefsd),
        _ => emit_line(crate::markers::M_SELFTEST_OTA_FALLBACK_FAIL),
    }
}

fn boot1_stage_and_switch(statefsd: &KernelClient) -> ! {
    let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
    let Ok(updated_client) = route_with_retry("updated") else {
        emit_line(crate::markers::M_SELFTEST_OTA_FALLBACK_FAIL);
        park_loud()
    };
    let (reply_send_slot, reply_recv_slot) = reply_slots();

    // The REAL container: the trial boots run these bytes far enough for
    // the loader to verify and jump them — the brick is init's honest
    // fault-fixture park, not a broken image.
    if updated::updated_stage_real(&updated_client, reply_send_slot, reply_recv_slot, &mut pending)
        .is_err()
    {
        emit_line(crate::markers::M_SELFTEST_OTA_FALLBACK_STAGE_FAIL);
        park_loud()
    }
    if updated::updated_switch(&updated_client, reply_send_slot, reply_recv_slot, 2, &mut pending)
        .is_err()
    {
        emit_line(crate::markers::M_SELFTEST_OTA_FALLBACK_STAGE_FAIL);
        park_loud()
    }
    emit_line(crate::markers::M_SELFTEST_OTA_FALLBACK_STAGED_OK);

    if write_sentinel(statefsd, b"w1").is_err() {
        emit_line(crate::markers::M_SELFTEST_OTA_FALLBACK_FAIL);
        park_loud()
    }
    reboot_now()
}

fn final_boot_verify(statefsd: &KernelClient) {
    let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
    let Ok(updated_client) = route_with_retry("updated") else {
        emit_line(crate::markers::M_SELFTEST_OTA_FALLBACK_FAIL);
        return;
    };
    let (reply_send_slot, reply_recv_slot) = reply_slots();

    // The rollback observation is part of bootctld's ATTACH — poll the
    // status to a deadline (wait-loop doctrine: bounded, timeout = honest
    // FAIL). The verdict wants active=a with NOTHING pending: a leftover
    // pending trial means the observation did not land and the next
    // commit would re-arm the broken image.
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(15_000_000_000);
    let mut settled = false;
    while nexus_abi::nsec().unwrap_or(u64::MAX) < deadline {
        if matches!(
            updated::updated_get_status(
                &updated_client,
                reply_send_slot,
                reply_recv_slot,
                &mut pending,
            ),
            Ok((updated::SlotId::A, None, _tries, _health))
        ) {
            settled = true;
            break;
        }
        let _ = nexus_abi::yield_();
    }

    // Cross-check against the LOADER's evidence: this boot must be the
    // fallback boot of slot a (measured record raw layout: present flag,
    // then slot at +10 — 0 = a).
    let mut measured = [0u8; 61];
    let measured_ok = matches!(
        bootctl_call_payload(&[b'B', b'T', 1, 12u8], 12, &mut measured),
        Some((0, len)) if len >= 61 && measured[0] == 1 && measured[11] == 0
    );

    let _ = del_sentinel(statefsd);
    if settled && measured_ok {
        emit_line(crate::markers::M_SELFTEST_OTA_FALLBACK_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_OTA_FALLBACK_FAIL);
    }
}

/// The selftest client's own CAP_MOVE reply inbox (slot_map SSOT; parity:
/// `otaflip.rs`).
fn reply_slots() -> (u32, u32) {
    (0x18, 0x17)
}

fn sentinel_phase(statefsd: &KernelClient) -> Option<u8> {
    let req = proto::encode_key_only_request(proto::OP_GET, SENTINEL_KEY).ok()?;
    let rsp = statefs_send_recv(statefsd, &req).ok()?;
    match proto::decode_get_response(&rsp) {
        Ok(value) if value == b"w1" => Some(1),
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

/// Boot 1 must never fall through into the ladder with a half-armed
/// trial: park visibly (the FAIL marker is already on the wire).
fn park_loud() -> ! {
    loop {
        let _ = nexus_abi::yield_();
    }
}
