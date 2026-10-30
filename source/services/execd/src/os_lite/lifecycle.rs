// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The lifecycle ORDERS execd carries out as the parent of every app process
//! (TASK-0333 6d, RFC-0111 §5, ADR-0082/0083): `OP_FREEZE` → `task_suspend`, `OP_THAW` →
//! `task_resume`, `OP_KILL` → `task_kill` (recorded first, so the reap reads the kernel's
//! `Killed` as the carried-out order and not as a crash), and (6f-1, ADR-0087) `OP_SQUEEZE` →
//! the batches of `task_squeeze` over the frozen child until the kernel's walk ends, the totals
//! answered (`nexus_wire::execd`). Two gates, both fail-closed: the sender must be the lifecycle
//! authority (abilitymgr's KERNEL sender id, never a payload string), and the pid must be a live
//! child of this service — an order for anything else is refused by name. Nothing here waits for
//! the child; a squeeze is execd's own syscalls, one bounded batch each.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: visible lane (`execd: freeze (pid=`, `execd: thaw (pid=`, `execd: squeeze
//!   (pid=`), the service launcher's probe (`SELFTEST: task kill ok`, `SELFTEST: exec child
//!   squeeze ok`); the gate is `crate::orders::order_allowed` (host-tested, `test_reject_*`)

use super::{rsp, State, STATUS_DENIED, STATUS_FAILED, STATUS_MALFORMED, STATUS_OK};
use alloc::vec::Vec;
use nexus_abi::{DebugLine, SqueezeBatch};
use nexus_wire::execd::{encode_squeeze_rsp, OP_SQUEEZE};

/// `[E,X,ver,OP_FREEZE, pid:u32le]` → `task_suspend(pid)`.
pub(super) const OP_FREEZE: u8 = 4;
/// `[E,X,ver,OP_THAW, pid:u32le]` → `task_resume(pid)`.
pub(super) const OP_THAW: u8 = 5;
/// `[E,X,ver,OP_KILL, pid:u32le]` → `task_kill(pid)`, an ORDERED kill (no crash report).
pub(super) const OP_KILL: u8 = 6;

/// Handles an order op (`OP_FREEZE..=OP_SQUEEZE`); `None` for any other op.
pub(super) fn handle_order(
    state: &mut State,
    sender_service_id: u64,
    op: u8,
    frame: &[u8],
) -> Option<Vec<u8>> {
    if !(OP_FREEZE..=OP_SQUEEZE).contains(&op) {
        return None;
    }
    // Order v1: [E,X,ver,op, pid:u32le]
    if frame.len() != 8 {
        return Some(reply(op, STATUS_MALFORMED, 0, SqueezeBatch::default()));
    }
    let pid = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let authority = nexus_abi::service_id_from_name(b"abilitymgr");
    let live_child = state.children.iter().any(|c| c.pid == pid && c.exit.is_none());
    if !crate::orders::order_allowed(sender_service_id, authority, live_child) {
        emit_pid_line("execd: order denied (sender or not a live child; pid=", pid);
        return Some(reply(op, STATUS_DENIED, pid, SqueezeBatch::default()));
    }
    if op == OP_SQUEEZE {
        return Some(match squeeze_child(pid) {
            Ok(squeeze) => {
                emit_squeeze_line(pid, &squeeze);
                reply(op, STATUS_OK, pid, squeeze.total)
            }
            Err(()) => {
                emit_pid_line("execd: FAIL squeeze (kernel refused; pid=", pid);
                reply(op, STATUS_FAILED, pid, SqueezeBatch::default())
            }
        });
    }
    let (label, done) = match op {
        OP_FREEZE => ("execd: freeze (pid=", nexus_abi::task_suspend(pid).is_ok()),
        OP_THAW => ("execd: thaw (pid=", nexus_abi::task_resume(pid).is_ok()),
        OP_KILL => {
            // Recorded BEFORE the order: the reap of a pid in this table is a carried-out
            // order, not a crash (ADR-0082). Forgotten when the kernel refused.
            let noted = crate::orders::note_ordered_kill(pid);
            let ok = noted && nexus_abi::task_kill(pid).is_ok();
            if noted && !ok {
                crate::orders::forget_ordered_kill(pid);
            }
            ("execd: kill (pid=", ok)
        }
        // The squeeze was answered above; no other op passes the range check.
        _ => return None,
    };
    if done {
        emit_pid_line(label, pid);
        Some(rsp(op, STATUS_OK, pid).to_vec())
    } else {
        emit_pid_line("execd: FAIL order (kernel refused; pid=", pid);
        Some(rsp(op, STATUS_FAILED, pid).to_vec())
    }
}

/// A whole squeeze: the batches' sum, how many there were, how long they took (the parent's
/// clock — what the memory authority and the lifecycle authority waited).
pub(super) struct Squeeze {
    pub(super) total: SqueezeBatch,
    pub(super) batches: u64,
    pub(super) us: u64,
}

/// Squeezes the parked child `pid` whole (ADR-0087): the batches of `task_squeeze` from `0`
/// until the kernel answers `0`, added up. A refused batch, or a cursor that does not move
/// forward, ends it as a failure — never a loop.
pub(super) fn squeeze_child(pid: u32) -> Result<Squeeze, ()> {
    let started = nexus_abi::nsec().unwrap_or(0);
    let mut squeeze = Squeeze { total: SqueezeBatch::default(), batches: 0, us: 0 };
    let mut from = 0usize;
    loop {
        let (next, batch) = nexus_abi::task_squeeze(pid as nexus_abi::Pid, from).map_err(|_| ())?;
        squeeze.total = squeeze.total.plus(batch);
        squeeze.batches += 1;
        if next == 0 {
            squeeze.us = nexus_abi::nsec().unwrap_or(started).saturating_sub(started) / 1000;
            return Ok(squeeze);
        }
        if next <= from {
            return Err(());
        }
        from = next;
    }
}

/// An order's reply: a squeeze carries its totals, the other orders are 9 bytes.
fn reply(op: u8, status: u8, pid: u32, total: SqueezeBatch) -> Vec<u8> {
    if op != OP_SQUEEZE {
        return rsp(op, status, pid).to_vec();
    }
    let n = |v: u64| u32::try_from(v).unwrap_or(u32::MAX);
    encode_squeeze_rsp(
        status,
        pid,
        n(total.squeezed),
        n(total.zero),
        n(total.kept),
        n(total.taken),
        n(total.stored),
    )
    .to_vec()
}

/// `execd: squeeze (pid= pages= zero= kept= taken= stored= batches= us=)` — one line, one write.
fn emit_squeeze_line(pid: u32, squeeze: &Squeeze) {
    let total = &squeeze.total;
    let mut line = DebugLine::new().put(b"execd: squeeze (pid=").u64(u64::from(pid));
    for (label, value) in [
        (" pages=", total.squeezed),
        (" zero=", total.zero),
        (" kept=", total.kept),
        (" taken=", total.taken),
        (" stored=", total.stored),
        (" batches=", squeeze.batches),
        (" us=", squeeze.us),
    ] {
        line = line.str(label).u64(value);
    }
    line.put(b")").emit();
}

/// `<prefix><pid>)` — one line, one write.
fn emit_pid_line(prefix: &str, pid: u32) {
    DebugLine::new().str(prefix).u64(u64::from(pid)).put(b")").emit();
}
