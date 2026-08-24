// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Real system-reset proof (TASK-0050 PR-3). Reset-lane only
//! (`fw_cfg selftest-profile=reset`, read RAW — proof boots keep the full
//! phase scope). Sentinel discipline over statefs, mirroring the cold-boot
//! probe: boot 1 finds no sentinel → writes it (PUT+SYNC) → asks bootctld
//! for an SBI cold reboot — QEMU restarts the SAME machine, the UART log
//! continues into a second boot (the launcher runs without `-no-reboot`).
//! Boot 2 finds the sentinel → deletes it → `SELFTEST: reset ok` — the
//! honest "we are the boot AFTER the reset" proof; never a simulated
//! "would have rebooted" print. Runs EARLY (bringup, right after the
//! statefs probes) so both boots fit one harness window.
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU reset lane (`--profile=reset`): two `init: ready`
//!   in ONE uart.log + request/ok markers, gated.
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

use core::time::Duration;

use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::KernelClient;
use statefs::protocol as proto;

use crate::markers::emit_line;
use crate::os_lite::services::statefs::statefs_send_recv;

const SENTINEL_KEY: &str = "/state/app/selftest/reset.proof";
const SENTINEL_VAL: &[u8] = b"reset-proof-v1";

/// Runs the reset proof; call only on the reset lane.
pub(crate) fn reset_proof(statefsd: &KernelClient) {
    match sentinel_present(statefsd) {
        Some(true) => {
            // We are the boot AFTER the reset: consume the sentinel so a
            // third boot (if any) never re-arms, then announce the proof.
            let _ = del_sentinel(statefsd);
            emit_line(crate::markers::M_SELFTEST_RESET_OK);
        }
        Some(false) => {
            if write_sentinel(statefsd).is_err() {
                emit_line(crate::markers::M_SELFTEST_RESET_REQUEST_FAIL);
                return;
            }
            emit_line(crate::markers::M_SELFTEST_RESET_REQUEST);
            request_reset();
            // A successful reset never returns — reaching the timeout means
            // the machine did NOT restart.
            let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(5_000_000_000);
            while nexus_abi::nsec().unwrap_or(u64::MAX) < deadline {
                let _ = nexus_abi::yield_();
            }
            emit_line(crate::markers::M_SELFTEST_RESET_REQUEST_FAIL);
        }
        None => emit_line(crate::markers::M_SELFTEST_RESET_REQUEST_FAIL),
    }
}

/// `Some(true)` sentinel present, `Some(false)` absent, `None` wire trouble.
fn sentinel_present(statefsd: &KernelClient) -> Option<bool> {
    let req = proto::encode_key_only_request(proto::OP_GET, SENTINEL_KEY).ok()?;
    let rsp = statefs_send_recv(statefsd, &req).ok()?;
    match proto::decode_get_response(&rsp) {
        Ok(_) => Some(true),
        Err(statefs::StatefsError::NotFound) => Some(false),
        Err(_) => None,
    }
}

fn write_sentinel(statefsd: &KernelClient) -> Result<(), ()> {
    let put = proto::encode_put_request(SENTINEL_KEY, SENTINEL_VAL).map_err(|_| ())?;
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

/// Fire the reset request at bootctld (fire-and-forget: on success the
/// machine restarts before any reply could arrive).
fn request_reset() {
    let send_slot = match budget::route_with_nonce_budgeted(
        b"bootctld",
        1,
        2,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, .. } => send_slot,
        _ => return,
    };
    let frame = [b'B', b'T', 1u8, 9u8, 0u8]; // OP_RESET, kind=reboot
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(2_000_000_000);
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => return,
            Err(nexus_abi::IpcError::QueueFull) => {
                if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
                    return;
                }
                let _ = nexus_abi::yield_();
            }
            Err(_) => return,
        }
    }
}
