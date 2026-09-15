// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: The fleet route ask (routing v1 + nonce over init's control channel, RFC-0093 §1)
//! and the raw blocking send every service uses for fire-and-forget frames. Nothing here
//! consults a clock (TASK-0324 P7, TASK-0054C P2-a): the deadline forms this module once
//! carried (`Clock`/`OsClock`/`HostClock`, `deadline_after`, `send_until`, `recv_until`,
//! `recv_matching_until`, `*_budgeted`) are deleted with `Wait::Timeout` — a wait ends when
//! the frame arrives or the peer dies, and `scripts/check-wait-not-poll.sh` keeps every
//! clock-bound form out of the tree. Request/reply lives in `crate::exchange`.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (crate public, but intended for in-tree use)
//! TEST_COVERAGE: QEMU — every route ask in the ladder runs through `route_with_nonce`

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use crate::IpcError;

/// Maximum number of nonce-mismatched replies tolerated while waiting for a correlated response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NonceMismatchBudget(u32);

impl NonceMismatchBudget {
    /// Creates a nonce-mismatch budget.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the raw budget value.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Outcome of a bounded routing attempt.
#[must_use = "routing outcomes must be handled explicitly"]
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteRetryOutcome {
    /// Route resolved successfully.
    Success {
        /// Send slot returned by samgrd routing.
        send_slot: u32,
        /// Receive slot returned by samgrd routing.
        recv_slot: u32,
    },
    /// ADR-0057: the target service is registered but currently DEAD (the
    /// supervisor marked it stale) and init could not park the ask. Callers
    /// keep the identity for diagnosis (ADR-0054: never collapse it into Rejected).
    TargetStale,
    /// Too many nonce mismatches were observed.
    NonceMismatchBudgetExceeded,
    /// The responder answered with a non-OK route status (`nexus_abi::routing::STATUS_*`),
    /// or the ask/answer was malformed (`STATUS_MALFORMED`). Carried so a status relay
    /// (bundlemgrd's `OP_ROUTE_STATUS`) needs no private copy of the exchange.
    Rejected {
        /// The route status the responder returned.
        status: u8,
    },
    /// Low-level IPC/runtime failure.
    Ipc(IpcError),
}

/// Resolves a service route (routing v1+nonce, RFC-0093 §1) over the fleet control channel
/// init installs in every child (`nexus_service_topology::CTRL_SLOTS`). ONE ask, WAITED for
/// without a deadline (TASK-0324 P7-b): init answers at once for a known target, PARKS the
/// ask while the target is not ready and answers when it is, and rejects an unknown name —
/// there is no fourth outcome a client clock could add. The nonce stays as correlation
/// sanity on the shared control stream (mandatory per RFC-0093 §1).
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub fn route_with_nonce(name: &[u8], mismatch_budget: NonceMismatchBudget) -> RouteRetryOutcome {
    use core::sync::atomic::{AtomicU32, Ordering};

    if name.is_empty() || name.len() > nexus_abi::routing::MAX_SERVICE_NAME_LEN {
        return RouteRetryOutcome::Rejected { status: nexus_abi::routing::STATUS_MALFORMED };
    }

    static ROUTE_NONCE: AtomicU32 = AtomicU32::new(1);
    let nonce = ROUTE_NONCE.fetch_add(1, Ordering::Relaxed);

    let mut req = [0u8; 5 + nexus_abi::routing::MAX_SERVICE_NAME_LEN + 4];
    let base_len = match nexus_abi::routing::encode_route_get(name, &mut req[..5 + name.len()]) {
        Some(v) => v,
        None => {
            return RouteRetryOutcome::Rejected { status: nexus_abi::routing::STATUS_MALFORMED }
        }
    };
    req[base_len..base_len + 4].copy_from_slice(&nonce.to_le_bytes());
    let req_len = base_len + 4;
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, req_len as u32);

    let ctrl = nexus_service_topology::CTRL_SLOTS;
    if let Err(e) = nexus_abi::ipc_send_v1(ctrl.send, &hdr, &req[..req_len], 0, 0) {
        return RouteRetryOutcome::Ipc(IpcError::Kernel(e));
    }

    let mut mismatches: u32 = 0;
    loop {
        // Every iteration WAITS in the kernel for the next control frame; a frame that is
        // not our answer is dropped and the wait resumes.
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        let n = match nexus_abi::ipc_recv_v1(
            ctrl.recv,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(v) => core::cmp::min(v as usize, buf.len()),
            Err(e) => return RouteRetryOutcome::Ipc(IpcError::Kernel(e)),
        };

        if n != 17 {
            continue;
        }
        let Some((status, send_slot, recv_slot)) = nexus_abi::routing::decode_route_rsp(&buf[..13])
        else {
            continue;
        };
        let got_nonce = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
        if got_nonce != nonce {
            mismatches = mismatches.saturating_add(1);
            if mismatches > mismatch_budget.raw() {
                return RouteRetryOutcome::NonceMismatchBudgetExceeded;
            }
            continue;
        }

        if status == nexus_abi::routing::STATUS_OK {
            return RouteRetryOutcome::Success { send_slot, recv_slot };
        }
        if status == nexus_abi::routing::STATUS_STALE {
            // RFC-0093 §1 (TASK-0324 P3): STALE is TERMINAL here. A dead-but-supervised
            // target is not answered at all any more — init PARKS the ask and answers it
            // once the supervisor re-provisions the route. The re-ask loop that used to
            // live here is exactly the client-side polling routing v2 deletes; a STALE that
            // still arrives means init could not park (overflow, loud on its side), and the
            // caller decides — never a hammering loop.
            return RouteRetryOutcome::TargetStale;
        }
        return RouteRetryOutcome::Rejected { status };
    }
}

/// Low-level helper for the kernel IPC v1 send syscall (slot + `MsgHeader`): a blocking
/// fire-and-forget send. OS-lite only, no allocations, no clock. Receives live in
/// `crate::exchange` (`recv_reply`, EOF-opted on the caller's own inbox).
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub mod raw {
    use crate::{IpcError, Result};

    fn map(err: nexus_abi::IpcError) -> IpcError {
        match err {
            nexus_abi::IpcError::QueueFull | nexus_abi::IpcError::QueueEmpty => {
                IpcError::WouldBlock
            }
            e => IpcError::Kernel(e),
        }
    }

    /// Blocks until `bytes` is queued on `send_slot` — no clock (TASK-0324 P7-d): queue space
    /// or the peer's death ends the wait.
    pub fn send_blocking(send_slot: u32, hdr: &nexus_abi::MsgHeader, bytes: &[u8]) -> Result<()> {
        nexus_abi::ipc_send_v1(send_slot, hdr, bytes, 0, 0).map(|_| ()).map_err(map)
    }
}
