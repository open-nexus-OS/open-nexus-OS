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
            // PR-4 target roundtrip: boot 1 armed next_boot=recovery; init
            // consumed it with the attempt ack (one-shot rides the same
            // persisted commit), so the authority must now read
            // (target=normal, next=none) — set → reset → consumed → clear.
            match bootctl_call(wire_op::GET_TARGET, None) {
                Some((0, payload)) if payload == [0, 0xff] => {
                    emit_line(crate::markers::M_SELFTEST_BOOT_TARGET_ROUNDTRIP_OK);
                }
                _ => emit_line(crate::markers::M_SELFTEST_BOOT_TARGET_ROUNDTRIP_FAIL),
            }
        }
        Some(false) => {
            if write_sentinel(statefsd).is_err() {
                emit_line(crate::markers::M_SELFTEST_RESET_REQUEST_FAIL);
                return;
            }
            // PR-4: arm the one-shot target BEFORE the reset (policy-gated
            // SET; the wire reject probe pins the malformed edge).
            let malformed_rejected =
                matches!(bootctl_call(wire_op::SET_NEXT_BOOT, Some(0x07)), Some((1, _)));
            let armed = matches!(bootctl_call(wire_op::SET_NEXT_BOOT, Some(1)), Some((0, _)));
            if !(malformed_rejected && armed) {
                emit_line(crate::markers::M_SELFTEST_BOOT_TARGET_ROUNDTRIP_FAIL);
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
