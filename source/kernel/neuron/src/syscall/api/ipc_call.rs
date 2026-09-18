// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `ipc_call` — request and reply in ONE trap (TASK-0054C P4a,
//! RFC-0096, ADR-0064).
//!
//! The client half of the fastpath. Today an exchange costs two user-initiated
//! traps (`ipc_send_v1`, then `ipc_recv_v1`) plus a scheduler hop for the wake.
//! This trap sends the request, blocks on the caller's OWN reply endpoint and —
//! for a reply of at most `IPC_SHORT_MAX` — never re-enters the kernel: the peer
//! finishes the syscall by writing `a0` and the payload registers into the
//! caller's saved frame (`task::completion`).
//!
//! NO DEADLINE, by ABI (RFC-0093 §7). Exactly two things end a call: the reply,
//! or the death of the last peer (RFC-0079 EOF → `EPIPE`).
//!
//! THE COMMIT POINT is the send. Every other blocking syscall here is
//! re-entrant — it blocks and the same instruction re-executes — but a call
//! whose request is already enqueued must never send it twice, so the task
//! carries a `CallState` from that moment. Phase 1 (validate + send) is still
//! fully re-entrant: a full target queue blocks in `IpcSend` with nothing
//! committed, exactly as `ipc_send_v1` does.
//!
//! The reply capability IS the wait target: the endpoint of the capability the
//! caller moves with its request is the endpoint it then waits on. A call that
//! moves no capability is refused — there would be nowhere for an answer to go.
//!
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Stable ABI (syscall 58)
//! TEST_COVERAGE: `crate::ipc_payload` (register packing, host) +
//!   `SELFTEST: ipc call ok` / `SELFTEST: ipc call eof ok` in QEMU
//! RFC: docs/rfcs/RFC-0096-ipc-performance-contract-v2-call-reply-recv-fastpath.md
//! ADR: docs/adr/0064-request-reply-one-trap-per-side-direct-handoff.md

use super::*;
use crate::cap::CapabilityKind;
use crate::ipc::header::MessageHeader;
use crate::task::completion::CallState;
use crate::task::BlockReason;

/// `ipc_call(send_slot, hdr_ptr, payload_ptr, payload_len, out_ptr, out_max)`.
///
/// Returns the reply length. A reply of at most `IPC_SHORT_MAX` arrives in
/// a1..a4 and `out` is untouched; a longer one is written to `out` (truncated to
/// `out_max`, with `a0` still the true length) by the re-executed syscall.
pub(super) fn sys_ipc_call(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let pid = ctx.tasks.current_pid();

    // Phase 3, the queued-reply tier: this call is already committed, so the
    // request must NOT be sent again. The answer is on the reply endpoint (or
    // its last peer died); collect it in our own address space.
    if let Some(state) = ctx.tasks.task(pid).and_then(|t| t.call_state()) {
        // The arguments are unchanged on a re-execution, so `out` is re-read here.
        return collect_queued_reply(ctx, pid, state, args.get(4), args.get(5));
    }

    let send_slot = args.get(0);
    let header_ptr = args.get(1);
    let payload_ptr = args.get(2);
    let payload_len = args.get(3);
    let out_ptr = args.get(4);
    let out_max = args.get(5);

    ensure_user_slice(header_ptr, 16)?;
    if payload_len > crate::ipc::IPC_PAYLOAD_MAX {
        return Err(Error::Ipc(ipc::IpcError::TooBig));
    }
    if payload_len != 0 {
        ensure_user_slice(payload_ptr, payload_len)?;
    }
    if out_max != 0 {
        ensure_user_slice(out_ptr, out_max)?;
    }

    // The reply endpoint is the endpoint of the capability being moved. Resolve
    // it BEFORE the send consumes the capability.
    let mut hdr_bytes = [0u8; 16];
    // SAFETY: `ensure_user_slice` above proved 16 readable bytes at `header_ptr`.
    unsafe {
        core::ptr::copy_nonoverlapping(header_ptr as *const u8, hdr_bytes.as_mut_ptr(), 16);
    }
    let user_hdr = MessageHeader::from_le_bytes(hdr_bytes);
    const IPC_MSG_FLAG_CAP_MOVE: u16 = 1 << 0;
    if (user_hdr.flags & IPC_MSG_FLAG_CAP_MOVE) == 0 {
        // A call with no reply capability has nowhere to be answered.
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let reply_ep = {
        let caps = ctx.tasks.current_caps_mut();
        let cap = caps
            .get(user_hdr.src as usize)
            .map_err(|_| Error::Ipc(ipc::IpcError::PermissionDenied))?;
        match cap.kind {
            CapabilityKind::Endpoint(id) => id,
            _ => return Err(AddressSpaceError::InvalidArgs.into()),
        }
    };

    // Phase 1 — re-entrant. The v1 send owns validation, CAP_MOVE, the payload
    // tier, the byte budgets and the receiver wake; duplicating any of that here
    // is how the two paths would drift. A full queue blocks in `IpcSend` and
    // this syscall re-executes with `call_state` still empty: nothing committed.
    let send_args = Args::new([send_slot, header_ptr, payload_ptr, payload_len, 0, 0]);
    super::ipc_msg::sys_ipc_send_v1(ctx, &send_args)?;

    // Phase 2 — commit. From here the syscall is never re-executed from the
    // start; the peer (or the EOF scan) finishes it.
    if let Some(task) = ctx.tasks.task_mut(pid) {
        task.set_call_state(CallState { wait_ep: reply_ep });
    }
    let _ = ctx.router.register_recv_waiter(reply_ep, pid.as_raw());
    ctx.tasks.block_current(BlockReason::IpcCall { reply_ep }, ctx.scheduler);
    if let Some(next) = ctx.scheduler.schedule_next() {
        ctx.tasks.set_current(next);
        return Err(Error::Reschedule);
    }
    // Nothing runnable on this hart: park it, the same way a blocking receive does.
    Err(park_hart_or_self_wake(ctx, pid, |ctx| {
        let _ = ctx.router.remove_recv_waiter(reply_ep, pid.as_raw());
    }))
}

/// The queued-reply tier: a reply too large for the registers stayed on the
/// endpoint and the caller was woken as an ordinary recv-waiter. Copy it out
/// here, in the caller's own address space, and close the call.
fn collect_queued_reply(
    ctx: &mut Context<'_>,
    pid: crate::task::Pid,
    state: CallState,
    out_ptr: usize,
    out_max: usize,
) -> SysResult<usize> {
    match ctx.router.recv(state.wait_ep) {
        Ok(msg) => {
            let total = msg.payload.len();
            let n = core::cmp::min(total, out_max);
            if n != 0 {
                // SAFETY: `out_ptr`/`out_max` were proved a user slice at the
                // commit point and this task owns that address space.
                unsafe {
                    core::ptr::copy_nonoverlapping(msg.payload.as_ptr(), out_ptr as *mut u8, n);
                }
                crate::ipc_stats::record_payload_copy(n);
            }
            if let Some(task) = ctx.tasks.task_mut(pid) {
                let _ = task.take_call_state();
            }
            Ok(total)
        }
        // Woken with nothing to take. RFC-0096 says exactly two things end a
        // call: the reply, or the death of the last peer. So decide with the
        // SAME host-tested predicate the receive path uses — inventing a second
        // EOF rule here is how the two would drift.
        Err(ipc::IpcError::QueueEmpty) => {
            let any_sender =
                super::eof_scan::foreign_sender_remains(ctx.tasks, ctx.router, state.wait_ep);
            if any_sender {
                ctx.router.mark_endpoint_had_sender(state.wait_ep);
            }
            let had_sender = ctx.router.endpoint_had_sender(state.wait_ep);
            if crate::ipc_eof::should_disconnect(true, had_sender, any_sender) {
                if let Some(task) = ctx.tasks.task_mut(pid) {
                    let _ = task.take_call_state();
                }
                return Err(Error::Ipc(ipc::IpcError::PeerClosed));
            }
            // Neither a frame nor an EOF: a spurious wake, or another reader
            // took the frame first. The call stays COMMITTED and waits again —
            // a call never ends on anything but its answer or a dead peer.
            let _ = ctx.router.register_recv_waiter(state.wait_ep, pid.as_raw());
            ctx.tasks
                .block_current(BlockReason::IpcCall { reply_ep: state.wait_ep }, ctx.scheduler);
            if let Some(next) = ctx.scheduler.schedule_next() {
                ctx.tasks.set_current(next);
                return Err(Error::Reschedule);
            }
            Err(park_hart_or_self_wake(ctx, pid, |ctx| {
                let _ = ctx.router.remove_recv_waiter(state.wait_ep, pid.as_raw());
            }))
        }
        Err(err) => {
            if let Some(task) = ctx.tasks.task_mut(pid) {
                let _ = task.take_call_state();
            }
            Err(Error::Ipc(err))
        }
    }
}

/// Finish a committed `ipc_call` whose answer just landed, when that answer
/// fits the return registers (TASK-0054C P4a).
///
/// Called from the send path with the message already enqueued on `endpoint`.
/// If `pid` is a call waiter on exactly this endpoint and the head message fits
/// `IPC_SHORT_MAX`, the message is taken here and the caller's syscall is
/// finished in its saved frame — so the caller resumes straight into user code
/// with the reply in a1..a4 and never traps again. Anything else is left
/// untouched: the ordinary wake follows and the re-executed syscall collects a
/// queued reply itself.
pub(super) fn complete_call_waiter_if_any(
    ctx: &mut Context<'_>,
    pid: crate::task::Pid,
    endpoint: ipc::EndpointId,
) {
    let waits_here = matches!(
        ctx.tasks.task(pid).and_then(|t| t.block_reason()),
        Some(BlockReason::IpcCall { reply_ep }) if reply_ep == endpoint
    );
    if !waits_here {
        return;
    }
    // Peek: only take the message if it can go home in registers.
    let fits = ctx
        .router
        .peek_payload_len(endpoint)
        .is_some_and(|len| len <= crate::ipc_payload::IPC_SHORT_MAX);
    if !fits {
        return;
    }
    let Ok(msg) = ctx.router.recv(endpoint) else {
        return;
    };
    let len = msg.payload.len();
    let regs = crate::ipc_payload::pack_reply_regs(msg.payload.as_slice());
    crate::ipc_stats::record_payload_copy(len);
    crate::ipc_stats::record_call_in_regs();
    if let Some(task) = ctx.tasks.task_mut(pid) {
        let _ = task.take_call_state();
        crate::task::completion::complete_in_frame(task.frame_mut(), len, regs);
    }
}

/// `ipc_reply_recv(reply_slot, hdr_ptr, payload_ptr, payload_len, recv_desc_ptr)`
/// — answer the current request and wait for the next one, in ONE trap
/// (TASK-0054C P4b, syscall 59).
///
/// The server half of the fastpath. A reply is already an ordinary send on the
/// moved capability's slot and the next request is already `ipc_recv_v2`'s
/// descriptor, so this is those two with P4a's commit rule between them — and
/// because the first half IS the send path, P4a's completion hook fires from
/// here unchanged: a client blocked in `ipc_call` gets its answer in registers
/// while this server is still inside its own single trap. Client one trap,
/// server one trap, no third mechanism.
///
/// The commit point matters for the same reason it does in `ipc_call`: once the
/// reply is out, a re-execution must not send it twice. `CallState` says so,
/// and the receive half re-reads its own descriptor from the unchanged
/// arguments.
pub(super) fn sys_ipc_reply_recv(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let pid = ctx.tasks.current_pid();
    let recv_args = Args::new([args.get(4), 0, 0, 0, 0, 0]);

    // The reply is already out (this is a re-execution after the receive
    // blocked): go straight to the receive.
    if ctx.tasks.task(pid).and_then(|t| t.call_state()).is_some() {
        let result = super::ipc_recv_v2::sys_ipc_recv_v2(ctx, &recv_args);
        if !matches!(result, Err(Error::Reschedule)) {
            if let Some(task) = ctx.tasks.task_mut(pid) {
                let _ = task.take_call_state();
            }
        }
        return result;
    }

    // Phase 1 — re-entrant: the reply, on the moved capability's slot. A full
    // client queue blocks in `IpcSend` with nothing committed, exactly as an
    // ordinary send does.
    let reply_slot = args.get(0);
    let send_args = Args::new([reply_slot, args.get(1), args.get(2), args.get(3), 0, 0]);
    super::ipc_msg::sys_ipc_send_v1(ctx, &send_args)?;
    // One-shot, like `ReplyCap::reply_and_close`: the answer is delivered, the
    // capability has no second use.
    let _ = ctx.tasks.current_caps_mut().take(reply_slot);

    // Phase 2 — commit, then the receive. If it blocks, the re-executed syscall
    // takes the branch above instead of replying again.
    if let Some(task) = ctx.tasks.task_mut(pid) {
        task.set_call_state(CallState { wait_ep: 0 });
    }
    let result = super::ipc_recv_v2::sys_ipc_recv_v2(ctx, &recv_args);
    if !matches!(result, Err(Error::Reschedule)) {
        if let Some(task) = ctx.tasks.task_mut(pid) {
            let _ = task.take_call_state();
        }
    }
    result
}
