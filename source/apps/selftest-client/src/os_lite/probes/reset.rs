// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Real system-reset + boot-target cycle proof (TASK-0050
//! PR-3..5). Reset-lane only (`fw_cfg selftest-profile=reset`, read RAW).
//! THREE boots in one uart stream, phased by a statefs sentinel VALUE:
//! boot 1 (normal) arms `next_boot=recovery` + sentinel `p1` → SBI
//! reboot; boot 2 comes up on the RECOVERY graph (core-only resume set —
//! `SELFTEST: recovery graph reached`), stamps `p2` → reboots again;
//! boot 3 (normal, one-shot long consumed) deletes the sentinel →
//! `SELFTEST: reset ok` + target roundtrip + `SELFTEST: recovery cycle
//! ok`, then the full ladder runs. Runs FIRST in bringup: a recovery
//! boot must never reach the full-graph probes (timed/imed are
//! suspended there), and the early exit keeps all three boots inside one
//! harness window.
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

/// Runs the phased reset/target cycle; call only on the reset lane.
/// Phases 0 and 1 END IN A REBOOT (they never return); phase 2 returns
/// and the normal ladder continues.
pub(crate) fn reset_proof(statefsd: &KernelClient) {
    match sentinel_phase(statefsd) {
        // Boot 1 (normal): arm the one-shot recovery target + phase stamp.
        Some(0) => {
            // Wire reject probe pins the malformed edge; then arm recovery.
            let malformed_rejected =
                matches!(bootctl_call(wire_op::SET_NEXT_BOOT, Some(0x07)), Some((1, _)));
            let armed = matches!(bootctl_call(wire_op::SET_NEXT_BOOT, Some(1)), Some((0, _)));
            if !(malformed_rejected && armed) {
                emit_line(crate::markers::M_SELFTEST_BOOT_TARGET_ROUNDTRIP_FAIL);
            }
            reboot_with_stamp(statefsd, b"p1");
        }
        // Boot 2: the RECOVERY graph (core-only resume set). Prove we got
        // here, stamp phase 2, go back to normal (next_boot was one-shot —
        // already consumed, so a plain reboot lands on `normal`).
        Some(1) => {
            emit_line(crate::markers::M_SELFTEST_RECOVERY_GRAPH_REACHED);
            reboot_with_stamp(statefsd, b"p2");
        }
        // Boot 3 (normal again): the cycle closed — consume the sentinel,
        // prove the roundtrip cleared, continue with the full ladder.
        Some(2) => {
            let _ = del_sentinel(statefsd);
            emit_line(crate::markers::M_SELFTEST_RESET_OK);
            match bootctl_call(wire_op::GET_TARGET, None) {
                Some((0, payload)) if payload == [0, 0xff] => {
                    emit_line(crate::markers::M_SELFTEST_BOOT_TARGET_ROUNDTRIP_OK);
                }
                _ => emit_line(crate::markers::M_SELFTEST_BOOT_TARGET_ROUNDTRIP_FAIL),
            }
            emit_line(crate::markers::M_SELFTEST_RECOVERY_CYCLE_OK);
        }
        _ => emit_line(crate::markers::M_SELFTEST_RESET_REQUEST_FAIL),
    }
}

/// Stamp the phase sentinel durably, request the reboot, and fail LOUD if
/// the machine is still alive after the budget (a successful SBI reset
/// never returns).
fn reboot_with_stamp(statefsd: &KernelClient, stamp: &[u8]) {
    if write_sentinel(statefsd, stamp).is_err() {
        emit_line(crate::markers::M_SELFTEST_RESET_REQUEST_FAIL);
        return;
    }
    emit_line(crate::markers::M_SELFTEST_RESET_REQUEST);
    request_reset();
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(5_000_000_000);
    while nexus_abi::nsec().unwrap_or(u64::MAX) < deadline {
        let _ = nexus_abi::yield_();
    }
    emit_line(crate::markers::M_SELFTEST_RESET_REQUEST_FAIL);
}

/// Phase from the sentinel VALUE: `Some(0)` absent, `Some(1)` = `p1`,
/// `Some(2)` = `p2`; `None` = wire trouble or an unknown stamp (fail
/// loud, never guess a phase).
fn sentinel_phase(statefsd: &KernelClient) -> Option<u8> {
    let req = proto::encode_key_only_request(proto::OP_GET, SENTINEL_KEY).ok()?;
    let rsp = statefs_send_recv(statefsd, &req).ok()?;
    match proto::decode_get_response(&rsp) {
        Ok(value) if value == b"p1" => Some(1),
        Ok(value) if value == b"p2" => Some(2),
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

/// bootctld wire op bytes used by this probe.
mod wire_op {
    pub const GET_TARGET: u8 = 6;
    pub const SET_NEXT_BOOT: u8 = 7;
    pub const RESET: u8 = 9;
}

/// Selftest CAP_MOVE reply inbox (deterministic wiring slots).
const REPLY_RECV_SLOT: u32 = 0x17;
const REPLY_SEND_SLOT: u32 = 0x18;

/// One bounded request/reply exchange with bootctld; returns
/// `(status, first-two payload bytes)` (missing bytes read as 0xff).
fn bootctl_call(op: u8, arg: Option<u8>) -> Option<(u8, [u8; 2])> {
    let send_slot = route_bootctld()?;
    let mut frame = [b'B', b'T', 1u8, op, 0u8];
    let len = match arg {
        Some(byte) => {
            frame[4] = byte;
            5
        }
        None => 4,
    };
    let reply_send_clone = nexus_abi::cap_clone(REPLY_SEND_SLOT).ok()?;
    let hdr =
        nexus_abi::MsgHeader::new(reply_send_clone, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, len as u32);
    let deadline = nexus_abi::nsec().ok()?.saturating_add(2_000_000_000);
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, &frame[..len], nexus_abi::IPC_SYS_NONBLOCK, 0)
        {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
                    let _ = nexus_abi::cap_close(reply_send_clone);
                    return None;
                }
                let _ = nexus_abi::yield_();
            }
            Err(_) => {
                let _ = nexus_abi::cap_close(reply_send_clone);
                return None;
            }
        }
    }
    loop {
        if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
            return None;
        }
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        match nexus_abi::ipc_recv_v1(
            REPLY_RECV_SLOT,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if n >= 7 && buf[0] == b'B' && buf[1] == b'T' && buf[3] == (op | 0x80) {
                    let p0 = buf.get(7).copied().unwrap_or(0xff);
                    let p1 = buf.get(8).copied().unwrap_or(0xff);
                    return Some((buf[4], [p0, p1]));
                }
                // Foreign inbox frame (logd/statefs acks): consumed, skipped.
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => return None,
        }
    }
}

fn route_bootctld() -> Option<u32> {
    match budget::route_with_nonce_budgeted(
        b"bootctld",
        1,
        2,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, .. } => Some(send_slot),
        _ => None,
    }
}

/// Fire the reset request at bootctld (fire-and-forget: on success the
/// machine restarts before any reply could arrive).
fn request_reset() {
    let Some(send_slot) = route_bootctld() else { return };
    let frame = [b'B', b'T', 1u8, wire_op::RESET, 0u8]; // kind=reboot
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
