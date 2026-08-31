// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: updated's bootctld client (TASK-0050 PR-2, ADR-0055). updated
//! no longer owns the boot record — every slot mutation delegates to the
//! single boot-state authority over the bootctld wire (`B`,`T`; ops mirror
//! the old in-process semantics 1:1). Route is resolved once via the init
//! responder and cached; replies ride updated's OWN CAP_MOVE inbox
//! (deterministic slots 0x0A/0x0B) — never a shared response queue.
//! Foreign frames on the inbox (statefs/logd acks) are skipped by magic +
//! op echo; updated keeps at most ONE bootctld call in flight, so no
//! nonce is needed on this wire yet.
//! OWNERS: @services-team @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU OTA ladder (markers unchanged, relocated
//!   authority): `SELFTEST: ota stage|switch|health|rollback ok`,
//!   `SELFTEST: bootctl persist ok`.
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;

use bootctld::wire;
use nexus_abi::MsgHeader;
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};

/// init-lite control-channel slots (route requests via the responder).
const CTRL_SEND_SLOT: u32 = 1;
const CTRL_RECV_SLOT: u32 = 2;
/// updated's deterministic CAP_MOVE reply inbox (slot_map SSOT).
const REPLY_RECV_SLOT: u32 = 0x0a;
const REPLY_SEND_SLOT: u32 = 0x0b;
/// Per-call wire budget.
const CALL_BUDGET_NS: u64 = 2_000_000_000;

/// A decoded bootctld reply: wire status + up to 20 payload bytes
/// (GET_STATUS grew additive tails: TASK-0036-B projection, TASK-0179
/// floor).
pub(crate) struct BootctlReply {
    pub status: u8,
    pub payload: [u8; 20],
    pub payload_len: usize,
}

/// One bounded request/reply exchange with bootctld. `None` = transport
/// trouble (route/send/recv) — the caller maps it to its FAILED audit.
pub(crate) fn call(op: u8, arg: Option<u8>) -> Option<BootctlReply> {
    match arg {
        Some(byte) => call_with_args(op, &[byte]),
        None => call_with_args(op, &[]),
    }
}

/// Like [`call`], with an arbitrary bounded arg tail (OP_STAGE carries the
/// staged rollback index as 4 LE bytes — TASK-0179 §10 commit rung).
pub(crate) fn call_with_args(op: u8, args: &[u8]) -> Option<BootctlReply> {
    let send_slot = cached_send_slot()?;
    let mut frame = [0u8; 12];
    if args.len() > 8 {
        return None;
    }
    frame[0] = wire::MAGIC0;
    frame[1] = wire::MAGIC1;
    frame[2] = wire::VERSION;
    frame[3] = op;
    frame[4..4 + args.len()].copy_from_slice(args);
    let len = 4 + args.len();
    let reply_send_clone = nexus_abi::cap_clone(REPLY_SEND_SLOT).ok()?;
    let hdr = MsgHeader::new(reply_send_clone, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, len as u32);
    let start = nexus_abi::nsec().ok()?;
    let deadline = start.saturating_add(CALL_BUDGET_NS);
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
    // Receive: skip foreign inbox traffic (statefs/logd acks) by magic +
    // the exact op echo; bounded by the call budget.
    loop {
        if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
            return None;
        }
        let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
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
                if n >= 7
                    && buf[0] == wire::MAGIC0
                    && buf[1] == wire::MAGIC1
                    && buf[2] == wire::VERSION
                    && buf[3] == (op | 0x80)
                {
                    let payload_len =
                        core::cmp::min(u16::from_le_bytes([buf[5], buf[6]]) as usize, 20);
                    let mut payload = [0u8; 20];
                    let avail = core::cmp::min(payload_len, n.saturating_sub(7));
                    payload[..avail].copy_from_slice(&buf[7..7 + avail]);
                    return Some(BootctlReply { status: buf[4], payload, payload_len: avail });
                }
                // Foreign frame: consumed and discarded (our inbox, our mess).
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => return None,
        }
    }
}

fn cached_send_slot() -> Option<u32> {
    static SEND: AtomicU32 = AtomicU32::new(0);
    let cached = SEND.load(Ordering::Relaxed);
    if cached != 0 {
        return Some(cached);
    }
    match budget::route_with_nonce_budgeted(
        b"bootctld",
        CTRL_SEND_SLOT,
        CTRL_RECV_SLOT,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, .. } => {
            SEND.store(send_slot, Ordering::Relaxed);
            Some(send_slot)
        }
        _ => None,
    }
}
