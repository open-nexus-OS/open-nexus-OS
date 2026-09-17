// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the state that lets a syscall be COMMITTED and finished later by
//! somebody else (TASK-0054C P4a, RFC-0096).
//!
//! Every other blocking syscall in this kernel is re-entrant: it blocks, and
//! when the task runs again the same instruction re-executes from the start
//! (`SysError::Reschedule`, see `core/trap/handler.rs`). `ipc_call` cannot be:
//! once its request is enqueued, re-executing would send it twice. So the task
//! records a [`CallState`] at the commit point, and the peer that answers
//! finishes the syscall by writing the result into the caller's SAVED FRAME —
//! the precedent is `core/trap/phased.rs`, which already completes `vm_map`
//! that way.
//!
//! WHY THE FRAME AND NOT A COPY-OUT: a reply of at most `IPC_SHORT_MAX` rides
//! home in registers a1..a4 (syscall arguments arrive in a0–a5 and the result
//! leaves in a0, so those are free). Registers are address-space independent,
//! so the peer needs no SATP of the caller, no cross-address-space primitive,
//! and no hook in the resume path — which matters, because the resume path is
//! FIVE places (`handler.rs:174,250,716`, `fault.rs:434,454`) and a missed one
//! would silently truncate a reply. P3a measured that 72.3 %–93.1 % of all
//! messages fit that tier, so this is the common path, not a special case.
//!
//! A longer reply is left queued and the caller is woken as an ordinary
//! recv-waiter: its `CallState` survives, so the re-executed `ipc_call` skips
//! the send and receives in its own context. One mechanism, two tiers.
//!
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `crate::ipc_payload` (the register packing, host) + the
//!   kernel IPC ladder in QEMU
//! RFC: docs/rfcs/RFC-0096-ipc-performance-contract-v2-call-reply-recv-fastpath.md

use crate::ipc_payload::REPLY_REGS;

/// A syscall whose OUTBOUND half is already committed.
///
/// One field, because a syscall that returns `Reschedule` re-executes the same
/// instruction with the same registers: the re-run can re-read every argument
/// it was given. The one thing the arguments cannot say is "the send already
/// happened" — that is this.
///
/// Two users, told apart by the block reason: an `ipc_call` waiter blocks in
/// `BlockReason::IpcCall` and is completed in its frame by the peer; an
/// `ipc_reply_recv` waiter blocks in `BlockReason::IpcRecv` and finishes its
/// own receive when it re-executes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallState {
    /// The endpoint this task now waits on.
    pub wait_ep: crate::ipc::EndpointId,
}

/// Finishes a committed `ipc_call` in the caller's saved frame: advance past
/// the `ecall`, put `a0` in place, and — for a reply that fits — the payload in
/// a1..a4.
///
/// Safe to call only on a task that is BLOCKED: a running task's frame is
/// re-saved from its live registers when it next traps, which would lose this.
pub fn complete_in_frame(
    frame: &mut crate::trap::TrapFrame,
    a0: usize,
    regs: Option<[usize; REPLY_REGS]>,
) {
    frame.sepc = frame.sepc.wrapping_add(4);
    frame.x[10] = a0;
    if let Some(regs) = regs {
        for (i, value) in regs.iter().enumerate() {
            frame.x[11 + i] = *value;
        }
    }
}

impl super::Task {
    /// The committed `ipc_call` this task is waiting on, if any.
    pub fn call_state(&self) -> Option<CallState> {
        self.call_state
    }

    /// Records the commit point: from here the syscall is never re-executed.
    pub fn set_call_state(&mut self, state: CallState) {
        self.call_state = Some(state);
    }

    /// Consumes the commit record — the call is finished (answered, or dead).
    pub fn take_call_state(&mut self) -> Option<CallState> {
        self.call_state.take()
    }
}
