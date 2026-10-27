// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The lifecycle ORDERS execd carries out as the parent of every app process
//! (TASK-0333 6d, RFC-0111 §5, ADR-0082/0083): `OP_FREEZE` → `task_suspend`, `OP_THAW` →
//! `task_resume`, `OP_KILL` → `task_kill` (recorded first, so the reap reads the kernel's
//! `Killed` as the carried-out order and not as a crash). Two gates, both fail-closed: the
//! sender must be the lifecycle authority (abilitymgr's KERNEL sender id, never a payload
//! string), and the pid must be a live child of this service — an order for anything else is
//! refused by name. Nothing here waits for the child.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: visible lane (`execd: freeze (pid=`, `execd: thaw (pid=`), the service
//!   launcher's probe (`SELFTEST: task kill ok`); the gate is `crate::orders::order_allowed`
//!   (host-tested, `test_reject_*`)

use super::emit::{emit_line_no_nl, emit_u64};
use super::{rsp, State, STATUS_DENIED, STATUS_FAILED, STATUS_MALFORMED, STATUS_OK};
use nexus_abi::debug_putc;

/// `[E,X,ver,OP_FREEZE, pid:u32le]` → `task_suspend(pid)`.
pub(super) const OP_FREEZE: u8 = 4;
/// `[E,X,ver,OP_THAW, pid:u32le]` → `task_resume(pid)`.
pub(super) const OP_THAW: u8 = 5;
/// `[E,X,ver,OP_KILL, pid:u32le]` → `task_kill(pid)`, an ORDERED kill (no crash report).
pub(super) const OP_KILL: u8 = 6;

/// Handles an order op (`OP_FREEZE..=OP_KILL`); `None` for any other op.
pub(super) fn handle_order(
    state: &mut State,
    sender_service_id: u64,
    op: u8,
    frame: &[u8],
) -> Option<[u8; 9]> {
    if !(OP_FREEZE..=OP_KILL).contains(&op) {
        return None;
    }
    // Order v1: [E,X,ver,op, pid:u32le]
    if frame.len() != 8 {
        return Some(rsp(op, STATUS_MALFORMED, 0));
    }
    let pid = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let authority = nexus_abi::service_id_from_name(b"abilitymgr");
    let live_child = state.children.iter().any(|c| c.pid == pid && c.exit.is_none());
    if !crate::orders::order_allowed(sender_service_id, authority, live_child) {
        emit_pid_line("execd: order denied (sender or not a live child; pid=", pid);
        return Some(rsp(op, STATUS_DENIED, pid));
    }
    let (label, done) = match op {
        OP_FREEZE => ("execd: freeze (pid=", nexus_abi::task_suspend(pid).is_ok()),
        OP_THAW => ("execd: thaw (pid=", nexus_abi::task_resume(pid).is_ok()),
        _ => {
            // Recorded BEFORE the order: the reap of a pid in this table is a carried-out
            // order, not a crash (ADR-0082). Forgotten when the kernel refused.
            let noted = crate::orders::note_ordered_kill(pid);
            let ok = noted && nexus_abi::task_kill(pid).is_ok();
            if noted && !ok {
                crate::orders::forget_ordered_kill(pid);
            }
            ("execd: kill (pid=", ok)
        }
    };
    if done {
        emit_pid_line(label, pid);
        Some(rsp(op, STATUS_OK, pid))
    } else {
        emit_pid_line("execd: FAIL order (kernel refused; pid=", pid);
        Some(rsp(op, STATUS_FAILED, pid))
    }
}

/// `<prefix><pid>)` on one line, alloc-free.
fn emit_pid_line(prefix: &str, pid: u32) {
    emit_line_no_nl(prefix);
    emit_u64(u64::from(pid));
    let _ = debug_putc(b')');
    let _ = debug_putc(b'\n');
}
