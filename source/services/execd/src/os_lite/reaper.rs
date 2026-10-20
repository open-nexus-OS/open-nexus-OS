// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The reaper of record wakes on a death (TASK-0333 6a). RFC-0081 made execd sweep
//! its exited children — on its NEXT request, so a closed window's app-host stayed a zombie,
//! its address space held, until somebody launched something. Now the kernel writes one
//! `OP_CHILD_EXITED` frame per dead child into the declared child-death endpoint
//! (`child_exit_bind`, the shape of a timer's fire), a waitset over it and the server endpoint
//! is the loop's one wait, and the sweep runs on the wake. What the sweep reaps is reported
//! once to the lifecycle authority: an app child's death goes to abilitymgr as
//! `OP_INSTANCE_EXITED` — a one-way event over execd's declared route, never a request
//! execd waits on (abilitymgr may itself be waiting on execd's spawn reply).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `execd: reaper armed` / `execd: exit notified (pid= to=abilitymgr)` on every
//!   lane; the board's open/close series; `KSELFTEST: child exit notify ok` for the kernel half

use alloc::vec::Vec;

use nexus_abi::ExitReason;
use nexus_ipc::{timer::Waitset, Client as _, KernelClient, Wait};
use nexus_service_topology::slots::execd as topo;

use super::emit::emit_line;

/// abilitymgr's wire (`source/services/abilitymgr/src/protocol.rs`): a death as one frame.
const ABILITYMGR_MAGIC: [u8; 2] = [b'A', b'M'];
const ABILITYMGR_VERSION: u8 = 1;
const OP_INSTANCE_EXITED: u8 = 5;
/// Deaths abilitymgr could not take yet (its queue full): retried on the next pass, bounded —
/// the zombie table is the truth, the frame is the report.
const PENDING_MAX: usize = 16;

/// A reaped child the authority has not heard of yet.
#[derive(Clone, Copy)]
pub(super) struct Death {
    pub pid: u32,
    pub code: i32,
    pub reason: ExitReason,
}

/// The reactive reaper: the waitset the loop blocks on and the death endpoint it drains.
pub(super) struct Reaper {
    waitset: Waitset,
    deaths: KernelClient,
    abilitymgr: Option<(u32, u32)>,
    pending: Vec<Death>,
}

impl Reaper {
    /// Binds the declared child-death endpoint and builds the waitset over the server endpoint
    /// (member 0) and the death endpoint (member 1). `None` names why on the console; the loop
    /// then falls back to the blocking sweep of RFC-0081 (a slower reap, never a lost one).
    pub(super) fn arm() -> Option<Self> {
        if nexus_abi::child_exit_bind(Some(topo::CHILD_EXIT.send)).is_err() {
            emit_line("execd: FAIL child exit bind (sweep on request)");
            return None;
        }
        let Ok(waitset) = Waitset::over(&[topo::SERVER.recv, topo::CHILD_EXIT.recv]) else {
            emit_line("execd: FAIL child exit waitset (sweep on request)");
            let _ = nexus_abi::child_exit_bind(None);
            return None;
        };
        let Ok(deaths) = KernelClient::new_with_slots(topo::CHILD_EXIT.send, topo::CHILD_EXIT.recv)
        else {
            emit_line("execd: FAIL child exit client (sweep on request)");
            let _ = nexus_abi::child_exit_bind(None);
            return None;
        };
        emit_line("execd: reaper armed");
        Some(Self { waitset, deaths, abilitymgr: None, pending: Vec::new() })
    }

    /// Blocks until a request or a death is pending.
    pub(super) fn wait(&self) {
        let _ = self.waitset.wait();
    }

    /// Drains every death frame pending (each is a wake, not the truth): `true` when at least
    /// one child died since the last drain, so the caller sweeps.
    pub(super) fn drain(&self) -> bool {
        let mut frame = [0u8; 32];
        let mut any = false;
        while let Ok(n) = self.deaths.recv_into(Wait::NonBlocking, &mut frame) {
            any |= nexus_abi::decode_child_exited(&frame[..n]).is_some();
        }
        any
    }

    /// Reports an app child's death to the lifecycle authority (one-way; a full queue parks
    /// the report, never execd). Each report is one marker when it leaves.
    pub(super) fn report(&mut self, death: Death) {
        if self.pending.len() < PENDING_MAX {
            self.pending.push(death);
        } else {
            emit_line("execd: FAIL exit report dropped (queue)");
        }
        self.flush();
    }

    /// Sends what abilitymgr has not taken yet.
    pub(super) fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        if self.abilitymgr.is_none() {
            self.abilitymgr = super::route_ctrl(b"abilitymgr");
        }
        let Some((send_slot, recv_slot)) = self.abilitymgr else {
            emit_line("execd: FAIL exit report (no abilitymgr route)");
            self.pending.clear();
            return;
        };
        let Ok(client) = KernelClient::new_with_slots(send_slot, recv_slot) else { return };
        while let Some(death) = self.pending.first().copied() {
            let frame = encode_instance_exited(death);
            match client.send(&frame, Wait::NonBlocking) {
                Ok(_) => {
                    self.pending.remove(0);
                    let _ = nexus_abi::debug_println(&alloc::format!(
                        "execd: exit notified (pid={} code={} reason={} to=abilitymgr)",
                        death.pid,
                        death.code,
                        death.reason.label()
                    ));
                }
                Err(_) => break,
            }
        }
    }
}

/// `[A, M, ver, OP_INSTANCE_EXITED, pid: u32le, code: i32le, reason: u8]` — the reason byte is
/// the consumer-side tag (0 clean, 1 error, 2 fault, 3 killed, 4 unknown).
fn encode_instance_exited(death: Death) -> [u8; 13] {
    let mut out = [0u8; 13];
    out[0..2].copy_from_slice(&ABILITYMGR_MAGIC);
    out[2] = ABILITYMGR_VERSION;
    out[3] = OP_INSTANCE_EXITED;
    out[4..8].copy_from_slice(&death.pid.to_le_bytes());
    out[8..12].copy_from_slice(&death.code.to_le_bytes());
    out[12] = match death.reason {
        ExitReason::Clean => 0,
        ExitReason::Error => 1,
        ExitReason::Fault { .. } => 2,
        ExitReason::Killed => 3,
        ExitReason::Unknown => 4,
    };
    out
}
