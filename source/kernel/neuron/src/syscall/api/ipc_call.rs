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
//! OFF THE KERNEL LOCK (TASK-0330 P2, ADR-0102): both bodies run over
//! `ipc_ctx::Ipc`. The peer may answer on another hart before the caller sleeps,
//! so the caller looks at its reply endpoint itself after the send — an answer
//! found there goes home the same way (registers for a short one).
//!
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Stable ABI (syscall 58)
//! TEST_COVERAGE: `crate::ipc_payload` (register packing, host) +
//!   `SELFTEST: ipc call ok` / `SELFTEST: ipc call eof ok` in QEMU
//! RFC: docs/rfcs/RFC-0096-ipc-performance-contract-v2-call-reply-recv-fastpath.md
//! ADR: docs/adr/0064-request-reply-one-trap-per-side-direct-handoff.md

use super::ipc_ctx::{locked, Ipc, IpcResult, Outcome, Slept, Waits};
use super::ipc_msg::{fill_send_v1, send, send_v1};
use super::ipc_recv_v2::{fill_recv_v2, recv_v2};
use super::*;
use crate::cap::CapabilityKind;
use crate::ipc::header::MessageHeader;
use crate::task::completion::CallState;
use crate::task::BlockReason;

/// Whether the caller has committed its outbound half (a call's request, a reply).
fn committed(ipc: &Ipc<'_>) -> bool {
    ipc.rec.thread().call.is_some()
}

/// The table's entry of `ipc_call`: the caller's pages committed (ADR-0077), then the body.
pub(super) fn sys_ipc_call(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let (out_ptr, out_max) = (args.get(4), args.get(5));
    if !committed(&ctx.ipc()?) {
        fill_send_v1(ctx, &Args::new([args.get(0), args.get(1), args.get(2), args.get(3), 0, 0]))?;
        if out_max != 0 {
            ensure_user_range(ctx, out_ptr, out_max, UserAccess::Write)?;
        }
    } else if out_max != 0 {
        // The caller may have been frozen while it waited and its pages squeezed or swapped
        // since the commit point (ADR-0087/0088): `out` is made writable again before the reply
        // leaves its queue — a squeezed page comes back here, a swapped one parks this
        // re-execution (the call stays committed); any other refusal ends the call.
        match ensure_user_range(ctx, out_ptr, out_max, UserAccess::Write) {
            Ok(()) => {}
            Err(err @ Error::PageIn { .. }) => return Err(err),
            Err(err) => {
                ctx.ipc()?.rec.thread().call = None;
                return Err(err);
            }
        }
    }
    let out = call(&ctx.ipc()?, args);
    locked(ctx, out)
}

/// `ipc_call(send_slot, hdr_ptr, payload_ptr, payload_len, out_ptr, out_max)`.
///
/// Returns the reply length. A reply of at most `IPC_SHORT_MAX` arrives in
/// a1..a4 and `out` is untouched; a longer one is written to `out` (truncated to
/// `out_max`, with `a0` still the true length).
pub(super) fn call(ipc: &Ipc<'_>, args: &Args) -> IpcResult {
    let out = request(ipc, args);
    // The caller runs on: a time right its request lent is its own again (a reply that was
    // there before it waited woke nobody).
    if !matches!(out, Ok(Outcome::Blocked(_))) {
        crate::sched::loan::take_back(ipc.me);
    }
    out
}

/// The call's two phases: the request, then the answer.
fn request(ipc: &Ipc<'_>, args: &Args) -> IpcResult {
    let (out_ptr, out_max) = (args.get(4), args.get(5));
    // This call is already committed, so the request must NOT be sent again. The answer is
    // on the reply endpoint (or its last peer died); the arguments are unchanged on a
    // re-execution, so `out` is re-read here.
    let state = ipc.rec.thread().call.clone();
    if let Some(state) = state {
        let Some(reply) = state.reply else {
            // A reply's commit, not a call's: no syscall leaves that behind for this one.
            ipc.rec.thread().call = None;
            return Err(AddressSpaceError::InvalidArgs.into());
        };
        return collect(ipc, reply.id(), out_ptr, out_max);
    }

    // The round trip starts here, on this hart (`ipc_stats::record_call_round_trip`).
    let started_ns = ipc.timer.now();
    let hart = u16::try_from(crate::smp::cpu_current_id().as_index()).unwrap_or(u16::MAX);
    let send_slot = args.get(0);
    let header_ptr = args.get(1);
    if args.get(3) > crate::ipc::IPC_PAYLOAD_MAX {
        return Err(Error::Ipc(ipc::IpcError::TooBig));
    }

    // The reply endpoint is the endpoint of the capability being moved. Resolve
    // it BEFORE the send consumes the capability.
    let mut hdr_bytes = [0u8; 16];
    if !ipc.read(header_ptr, &mut hdr_bytes) {
        return Ok(Outcome::Locked);
    }
    let user_hdr = MessageHeader::from_le_bytes(hdr_bytes);
    const IPC_MSG_FLAG_CAP_MOVE: u16 = 1 << 0;
    if (user_hdr.flags & IPC_MSG_FLAG_CAP_MOVE) == 0 {
        // A call with no reply capability has nowhere to be answered.
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let endpoint_of = |slot: usize| ipc.caps().get(slot).map(|cap| cap.kind);
    let denied = Error::Ipc(ipc::IpcError::PermissionDenied);
    // Held from here until the call ends: the capability travels, the caller waits on.
    let reply = {
        let caps = ipc.caps();
        let slot = user_hdr.src as usize;
        match caps.get(slot).map(|cap| cap.kind) {
            Ok(CapabilityKind::Endpoint(_)) => {
                caps.pin_endpoint(slot, Rights::empty()).map_err(|_| denied)?
            }
            Ok(_) => return Err(AddressSpaceError::InvalidArgs.into()),
            Err(_) => return Err(denied),
        }
    };

    // ADR-0082: a call to a frozen receiver would wait for an answer only a thaw can bring —
    // refused at once (`Frozen`), like a send that would park on it.
    if let Ok(CapabilityKind::Endpoint(id)) = endpoint_of(send_slot) {
        if ipc.router.endpoint_frozen(id) {
            return Err(Error::Ipc(ipc::IpcError::Frozen));
        }
    }

    // Phase 1 — re-entrant. The v1 send owns validation, CAP_MOVE, the payload
    // tier, the byte budgets and the receiver wake; duplicating any of that here
    // is how the two paths would drift. A full queue blocks in `IpcSend` and
    // this syscall re-executes with nothing committed.
    let send_args = Args::new([send_slot, header_ptr, args.get(2), args.get(3), 0, 0]);
    match send(ipc, &send_args, true)? {
        Outcome::Done(_) => {}
        other => return Ok(other),
    }

    // Phase 2 — commit. From here the syscall is never re-executed from the
    // start; the peer (or the EOF scan) finishes it, or the caller finds the
    // answer already there.
    let endpoint = reply.id();
    ipc.rec.thread().call = Some(CallState { reply: Some(reply), started_ns, hart });
    collect(ipc, endpoint, out_ptr, out_max)
}

/// Whether the last peer of a committed call is gone — the SAME host-tested predicate the
/// receive path uses (inventing a second EOF rule here is how the two would drift).
fn peer_closed(router: &ipc::Router, endpoint: ipc::EndpointId) -> bool {
    let any_sender = super::eof_scan::foreign_sender_remains(router, endpoint);
    if any_sender {
        router.mark_endpoint_had_sender(endpoint);
    }
    crate::ipc_eof::should_disconnect(true, router.endpoint_had_sender(endpoint), any_sender)
}

/// A committed call takes its answer or waits for it. RFC-0096 says exactly two things end a
/// call: the reply, or the death of the last peer — a wake with neither (another reader took
/// the frame, a wake too many) leaves it committed and waiting.
fn collect(ipc: &Ipc<'_>, endpoint: ipc::EndpointId, out_ptr: usize, out_max: usize) -> IpcResult {
    let me = ipc.me.as_raw();
    let end = || ipc.rec.thread().call = None;
    loop {
        match ipc.router.recv(endpoint) {
            Ok(msg) => return take_home(ipc, endpoint, msg, out_ptr, out_max),
            Err(ipc::IpcError::QueueEmpty) => {}
            Err(err) => {
                end();
                return Err(Error::Ipc(err));
            }
        }
        if peer_closed(ipc.router, endpoint) {
            // The answer is queued before the peer lets go of the reply capability: a queue
            // that fills between the look above and this verdict is taken first.
            if ipc.router.pending(endpoint) {
                continue;
            }
            end();
            return Err(Error::Ipc(ipc::IpcError::PeerClosed));
        }
        ipc.prepare(BlockReason::IpcCall { reply_ep: endpoint });
        if let Err(err) = ipc.router.register_recv_waiter(endpoint, me) {
            ipc.abort();
            end();
            return Err(Error::Ipc(err));
        }
        // The answer, or the peer's death, may have come before the registration.
        if ipc.router.pending(endpoint) || peer_closed(ipc.router, endpoint) {
            ipc.abort();
            let _ = ipc.router.remove_recv_waiter(endpoint, me);
            continue;
        }
        match ipc.sleep() {
            Slept::Blocked => return Ok(Outcome::Blocked(Waits::Recv(endpoint))),
            Slept::Order => {
                let _ = ipc.router.remove_recv_waiter(endpoint, me);
                return Ok(Outcome::Locked);
            }
            Slept::Woken => {
                let _ = ipc.router.remove_recv_waiter(endpoint, me);
            }
        }
    }
}

/// The caller takes its own answer home: a short one in a1..a4 of its saved frame — the
/// registers the peer would have written (`complete_call_waiter_if_any`) — a longer one into
/// `out`, in the caller's own address space. A page of `out` not ready puts the answer back.
fn take_home(
    ipc: &Ipc<'_>,
    endpoint: ipc::EndpointId,
    msg: ipc::Message,
    out_ptr: usize,
    out_max: usize,
) -> IpcResult {
    let total = msg.payload.len();
    if let Some(regs) = crate::ipc_payload::pack_reply_regs(msg.payload.as_slice()) {
        let frame = ipc.rec.frame_mut();
        for (i, value) in regs.iter().enumerate() {
            frame.x[11 + i] = *value;
        }
        crate::ipc_stats::record_payload_copy(total);
    } else {
        let n = core::cmp::min(total, out_max);
        if n != 0 {
            if !ipc.write(out_ptr, &msg.payload.as_slice()[..n]) {
                let _ = ipc.router.requeue_front(endpoint, msg);
                return Ok(Outcome::Locked);
            }
            crate::ipc_stats::record_payload_copy(n);
        }
    }
    let state = ipc.rec.thread().call.take();
    if let Some(state) = state {
        let ns = ipc.timer.now().saturating_sub(state.started_ns);
        crate::ipc_stats::record_call_round_trip(usize::from(state.hart), ns);
    }
    Ok(Outcome::Done(total))
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
///
/// TASK-0330 P2 (ADR-0102): everything here is the waiter's scheduling record and the
/// endpoint — no task table. A blocked thread's frame is whoever holds its thread lock's: the
/// verdict "it waits here, blocked" and the write to its frame are one hold, and only a wake
/// under that lock makes it runnable. The endpoint's lock ranks below the thread's, so the
/// message is taken between two holds; a waiter that was woken in between (a kill, its last
/// peer's death) gets the message back at the head of its queue and collects it itself.
pub(super) fn complete_call_waiter_if_any(
    router: &ipc::Router,
    pid: crate::task::Pid,
    endpoint: ipc::EndpointId,
    now: u64,
) {
    use crate::sched::record::Thread;
    let Some(rec) = crate::sched::record::get(pid) else {
        return;
    };
    let waits_here = |thread: &Thread| {
        thread.is_blocked()
            && matches!(thread.reason, Some(BlockReason::IpcCall { reply_ep }) if reply_ep == endpoint)
    };
    if !waits_here(&rec.thread()) {
        return;
    }
    // Peek: only take the message if it can go home in registers.
    let short = |len: usize| len <= crate::ipc_payload::IPC_SHORT_MAX;
    if !router.peek_payload_len(endpoint).is_some_and(short) {
        return;
    }
    let Ok(msg) = router.recv(endpoint) else {
        return;
    };
    let len = msg.payload.len();
    let mut thread = rec.thread();
    if !short(len) || !waits_here(&thread) {
        drop(thread);
        let _ = router.requeue_front(endpoint, msg);
        return;
    }
    let regs = crate::ipc_payload::pack_reply_regs(msg.payload.as_slice());
    crate::ipc_stats::record_payload_copy(len);
    crate::ipc_stats::record_call_in_regs();
    if let Some(state) = thread.call.take() {
        let ns = now.saturating_sub(state.started_ns);
        crate::ipc_stats::record_call_round_trip(usize::from(state.hart), ns);
    }
    crate::task::completion::complete_in_frame(rec.frame_mut(), len, regs);
}

/// The table's entry of `ipc_reply_recv`: the caller's pages committed, then the body. A
/// refusal after the reply is out ends the committed state — the next syscall replies again.
pub(super) fn sys_ipc_reply_recv(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    if !committed(&ctx.ipc()?) {
        fill_send_v1(ctx, &reply_args(args))?;
    }
    if let Err(err) = fill_recv_v2(ctx, args.get(4)) {
        if !matches!(err, Error::PageIn { .. }) {
            ctx.ipc()?.rec.thread().call = None;
        }
        return Err(err);
    }
    let out = reply_recv(&ctx.ipc()?, args);
    locked(ctx, out)
}

/// The reply half's arguments as the v1 send takes them. The reply half is NON-BLOCKING, like
/// the `send_on_cap` it replaces. A server that waits out a client's full reply queue is the
/// 0049B wedge this task spent five packages removing — and here it would stop the server for
/// everyone, not just that client. A reply nobody can take is dropped, which is the rule
/// statefsd's restored `Wait::NonBlocking` enshrines.
fn reply_args(args: &Args) -> Args {
    Args::new([args.get(0), args.get(1), args.get(2), args.get(3), IPC_SYS_NONBLOCK, 0])
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
pub(super) fn reply_recv(ipc: &Ipc<'_>, args: &Args) -> IpcResult {
    // The reply is already out when this is a re-execution after the receive blocked: then
    // straight to the receive.
    if !committed(ipc) {
        // Phase 1 — re-entrant: the reply, on the moved capability's slot. A page of it not
        // ready: nothing is sent yet, the body asks for the lock.
        if matches!(send_v1(ipc, &reply_args(args)), Ok(Outcome::Locked)) {
            return Ok(Outcome::Locked);
        }
        // One-shot, like `ReplyCap::reply_and_close`: the answer is delivered, the
        // capability has no second use.
        let _ = ipc.caps().take(args.get(0));
        // Phase 2 — commit, then the receive. If it blocks, the re-executed syscall goes
        // straight to the receive instead of replying again.
        ipc.rec.thread().call = Some(CallState { reply: None, started_ns: 0, hart: 0 });
    }
    let out = recv_v2(ipc, args.get(4));
    if !matches!(out, Ok(Outcome::Blocked(_) | Outcome::Locked)) {
        ipc.rec.thread().call = None;
    }
    out
}
