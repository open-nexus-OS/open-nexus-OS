// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: IPC message-transfer syscalls split out of the former single-file
//! api.rs: legacy sys_send/sys_recv, the v1 send (incl. the CAP_MOVE
//! take/rollback path) and the v1 receive, plus their typed decoders. Each v1
//! syscall is one body over `ipc_ctx::Ipc` (TASK-0330 P2, ADR-0102) — run by the
//! trap's entry before the kernel lock, and by the table's entry under it,
//! which commits the caller's pages first.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: neuron host tests + QEMU marker gates (just test-os / ci-os-smp)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::ipc_ctx::{deliver, locked, take, Ipc, IpcResult, Out, Outcome, Slept, Taken, Waits};
use super::*;

// Typed decoders for the Decode→Check→Execute syscall discipline

#[derive(Copy, Clone)]
pub(super) struct SendArgsTyped {
    slot: SlotIndex,
    ty: u16,
    flags: u16,
    len: u32,
}

impl SendArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot: SlotIndex::decode(args.get(0)),
            ty: args.get(1) as u16,
            flags: args.get(2) as u16,
            len: args.get(3) as u32,
        })
    }
    #[inline]
    fn check(&self) -> Result<(), Error> {
        // Keep len unconstrained for now; stage-policy minimal checks
        let _ = self.len;
        Ok(())
    }
}

#[derive(Copy, Clone)]
pub(super) struct RecvArgsTyped {
    slot: SlotIndex,
}

impl RecvArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self { slot: SlotIndex::decode(args.get(0)) })
    }
    #[inline]
    fn check(&self) -> Result<(), Error> {
        Ok(())
    }
}

pub(super) const IPC_SYS_NONBLOCK: usize = 1 << 0;
pub(super) const IPC_SYS_TRUNCATE: usize = 1 << 1;
/// RFC-0079 opt-in: a blocking recv with this flag returns `PeerClosed`
/// instead of blocking when the endpoint has had a sender and now has none.
pub(super) const IPC_SYS_EOF: usize = 1 << 2;

#[derive(Copy, Clone)]
pub(super) struct IpcSendV1ArgsTyped {
    slot: SlotIndex,
    header_ptr: usize,
    payload_ptr: usize,
    payload_len: usize,
    sys_flags: usize,
    deadline_ns: u64,
}

impl IpcSendV1ArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot: SlotIndex::decode(args.get(0)),
            header_ptr: args.get(1),
            payload_ptr: args.get(2),
            payload_len: args.get(3),
            sys_flags: args.get(4),
            deadline_ns: args.get(5) as u64,
        })
    }

    /// E2BIG, not EINVAL: "your message is too big" is not "you passed a bad pointer", and
    /// userspace could not tell them apart before (TASK-0054C P3b, ADR-0054).
    #[inline]
    fn bound(&self) -> Result<(), Error> {
        if self.payload_len > crate::ipc::IPC_PAYLOAD_MAX {
            return Err(Error::Ipc(ipc::IpcError::TooBig));
        }
        Ok(())
    }

    /// Commits the pages the send reads (the locked entry, ADR-0077).
    #[inline]
    fn check(&self, ctx: &mut Context<'_>) -> Result<(), Error> {
        ensure_user_range(ctx, self.header_ptr, 16, UserAccess::Read)?;
        self.bound()?;
        if self.payload_len != 0 {
            ensure_user_range(ctx, self.payload_ptr, self.payload_len, UserAccess::Read)?;
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
pub(super) struct IpcRecvV1ArgsTyped {
    slot: SlotIndex,
    header_out_ptr: usize,
    payload_out_ptr: usize,
    payload_out_max: usize,
    sys_flags: usize,
    deadline_ns: u64,
}

impl IpcRecvV1ArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot: SlotIndex::decode(args.get(0)),
            header_out_ptr: args.get(1),
            payload_out_ptr: args.get(2),
            payload_out_max: args.get(3),
            sys_flags: args.get(4),
            deadline_ns: args.get(5) as u64,
        })
    }

    /// Commits the pages the receive writes (the locked entry, ADR-0077).
    #[inline]
    fn check(&self, ctx: &mut Context<'_>) -> Result<(), Error> {
        ensure_user_range(ctx, self.header_out_ptr, 16, UserAccess::Write)?;
        if self.payload_out_max != 0 {
            ensure_user_range(ctx, self.payload_out_ptr, self.payload_out_max, UserAccess::Write)?;
        }
        Ok(())
    }
}

pub(super) fn sys_send(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = SendArgsTyped::decode(args)?;
    typed.check()?;
    let endpoint =
        ctx.tasks.current_caps_mut().derive_endpoint_ref(typed.slot.0, Rights::SEND)?.endpoint();
    let header =
        MessageHeader::new(typed.slot.0 as u32, endpoint, typed.ty, typed.flags, typed.len);
    ctx.router.send(endpoint, ipc::Message::new(header, ipc::Payload::empty(), None))?;
    Ok(typed.len as usize)
}

pub(super) fn sys_recv(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = RecvArgsTyped::decode(args)?;
    typed.check()?;
    let endpoint =
        ctx.tasks.current_caps_mut().derive_endpoint_ref(typed.slot.0, Rights::RECV)?.endpoint();
    // The EOF latch is consumed on observed emptiness only (TASK-0054C P2-b, see recv v1).
    let message = match ctx.router.recv(endpoint) {
        Ok(m) => m,
        Err(e) => {
            if matches!(e, ipc::IpcError::QueueEmpty) {
                ctx.router.clear_eof_pending(endpoint);
            }
            return Err(e.into());
        }
    };
    let len = message.header.len as usize;
    ctx.last_message = Some(message);
    Ok(len)
}

/// Commits the pages a v1 send with `args` reads (ADR-0077).
pub(super) fn fill_send_v1(ctx: &mut Context<'_>, args: &Args) -> Result<(), Error> {
    IpcSendV1ArgsTyped::decode(args)?.check(ctx)
}

/// The table's entry of the v1 send: the caller's pages committed, then the body.
pub(super) fn sys_ipc_send_v1(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    fill_send_v1(ctx, args)?;
    let out = send_v1(&ctx.ipc()?, args);
    locked(ctx, out)
}

/// `ipc_send_v1(slot, hdr_ptr, payload_ptr, payload_len, sys_flags, deadline_ns)`.
pub(super) fn send_v1(ipc: &Ipc<'_>, args: &Args) -> IpcResult {
    send(ipc, args, false)
}

/// The send. `lends`: it is a call's request — the receiver it wakes runs with the caller's
/// time right until the caller runs again (`sched::loan`).
pub(super) fn send(ipc: &Ipc<'_>, args: &Args, lends: bool) -> IpcResult {
    let typed = IpcSendV1ArgsTyped::decode(args)?;
    typed.bound()?;
    if (typed.sys_flags & !(IPC_SYS_NONBLOCK)) != 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let nonblock = (typed.sys_flags & IPC_SYS_NONBLOCK) != 0;
    let me = ipc.me.as_raw();

    // Held for the body's length: the id stays this endpoint's (`cap::PinnedEndpoint`).
    let target = ipc.caps().pin_endpoint(typed.slot.0, Rights::SEND)?;
    let endpoint = target.id();

    let mut hdr_bytes = [0u8; 16];
    if !ipc.read(typed.header_ptr, &mut hdr_bytes) {
        return Ok(Outcome::Locked);
    }
    let user_hdr = MessageHeader::from_le_bytes(hdr_bytes);

    // IPC v1 extension: move one capability alongside the message.
    // When set, `user_hdr.src` is treated as a cap slot in the sender, which is consumed (taken)
    // and delivered to the receiver. On receive, `header_out.src` is overwritten with the newly
    // allocated cap slot in the receiver.
    const IPC_MSG_FLAG_CAP_MOVE: u16 = 1 << 0;
    let cap_move = (user_hdr.flags & IPC_MSG_FLAG_CAP_MOVE) != 0;

    // Enforce header/payload agreement.
    if user_hdr.len as usize != typed.payload_len {
        return Err(AddressSpaceError::InvalidArgs.into());
    }

    // The payload lands in whichever tier fits it: up to IPC_SHORT_MAX it rides inside the
    // message and the kernel heap is never touched (TASK-0054C P3b). Copied before anything of
    // the sender's changes: a page not ready ends the body here.
    let Some(payload) =
        ipc::Payload::filled(typed.payload_len, |buf| ipc.read(typed.payload_ptr, buf))
    else {
        return Ok(Outcome::Locked);
    };
    if payload.is_heap() {
        crate::ipc_stats::record_payload_alloc(typed.payload_len);
    }
    if typed.payload_len != 0 {
        crate::ipc_stats::record_payload_copy(typed.payload_len);
    }
    // The size distribution, once per message: it sized this tier (TASK-0054C P3a).
    crate::ipc_stats::record_payload_size(typed.payload_len);

    let cap_move_slot = if cap_move { Some(user_hdr.src as usize) } else { None };

    // Sender attribution: kernel sets `dst` to the sender PID so receivers can attribute messages.
    // `src` is reserved for CAP_MOVE return value on receive.
    let header = MessageHeader::new(0, me, user_hdr.ty, user_hdr.flags, typed.payload_len as u32);

    if !nonblock && typed.deadline_ns != 0 {
        crate::trap::arm_wakeup(ipc.timer, typed.deadline_ns);
    }
    // The capability a refused attempt hands back returns to its slot.
    let give_back = |msg: &mut ipc::Message| {
        if let (Some(slot), Some(held)) = (cap_move_slot, msg.moved_cap.take()) {
            let _ = ipc.caps().set(slot, held);
        }
    };
    let mut payload = Some(payload);
    loop {
        // If CAP_MOVE is set, the capability leaves the slot for this attempt — judged where
        // it sits, taken only once it may travel. The capability itself moves (RFC-0102): its
        // kind and rights are what the sender held, and its object counts it the whole way.
        // If the attempt fails (QueueFull, NoSuchEndpoint, etc.) the message hands it back and
        // it returns to its slot before returning/rescheduling.
        // Judged and taken under ONE hold of the table's lock: a thread of the same process on
        // another hart cannot put a different capability into the slot between the two.
        let moved_cap = if let Some(slot) = cap_move_slot {
            let mut caps = ipc.caps();
            let cap = caps.get(slot)?;
            // Security floor: never allow moving MANAGE authority in-band.
            if cap.rights.contains(Rights::MANAGE) || cap.kind == CapabilityKind::EndpointFactory {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            // Hardening: do not allow CAP_MOVE of dead/non-existent endpoints.
            if let CapabilityKind::Endpoint(id) = cap.kind {
                if !ipc.router.endpoint_alive(id) {
                    return Err(Error::Ipc(ipc::IpcError::NoSuchEndpoint));
                }
                #[cfg(feature = "ipc_trace_ring")]
                crate::ipc::trace::record_capmove_send(
                    me,
                    typed.slot.0 as u16,
                    slot as u16,
                    endpoint,
                    id,
                );
            }
            Some(caps.take_held(slot)?)
        } else {
            None
        };
        // The payload MOVES in (TASK-0054C P3a): a failed send hands the whole `Message` back
        // via `Err((IpcError, Message))`, and a round that looks again takes it from there.
        let mut msg = ipc::Message::new(header, payload.take().unwrap_or_default(), moved_cap);
        msg.sender_service_id = ipc.rec.service_id();
        #[cfg(feature = "ipc_trace_ring")]
        crate::ipc::trace::record_send(
            me,
            typed.slot.0 as u16,
            endpoint,
            msg.header.flags,
            msg.payload.len() as u16,
            None,
        );
        #[cfg(feature = "debug_uart")]
        {
            let p = msg.payload.as_slice();
            if p.len() >= 4 && p[0] == b'S' && p[1] == b'M' && p[2] == 1 && p[3] == 1 {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = writeln!(u, "IPC-SEND samgr reg ep=0x{:x}", endpoint);
            }
        }
        match ipc.router.send_returning_message(endpoint, msg) {
            Ok(()) => {
                crate::ipc_stats::record_send(crate::smp::cpu_current_id().as_index());
                // Wake one receiver blocked on this endpoint (if any).
                if let Ok(Some(waiter)) = ipc.router.pop_recv_waiter(endpoint) {
                    let pid = task::Pid::from_raw(waiter);
                    // A committed `ipc_call` waiter is finished HERE when the reply fits the
                    // return registers (TASK-0054C P4a); anything longer stays queued.
                    super::ipc_call::complete_call_waiter_if_any(
                        ipc.router,
                        pid,
                        endpoint,
                        ipc.timer.now(),
                    );
                    let here = crate::smp::cpu_current_id();
                    let cross = crate::sched::record::get(pid).is_some_and(|r| r.home() != here);
                    crate::ipc_stats::record_recv_wake(cross);
                    if lends {
                        crate::sched::loan::lend(ipc.me, pid);
                    }
                    ipc.wake(waiter);
                }
                // Low-noise triage: dump trace ring once on the first "large CAP_MOVE" send.
                // This helps diagnose OTA stage hangs without relying on NoSuchEndpoint spam.
                #[cfg(feature = "ipc_trace_ring")]
                if cap_move && typed.payload_len > 1024 {
                    crate::ipc::trace::maybe_dump_capmove_big("capmove-big");
                }
                return Ok(Outcome::Done(typed.payload_len));
            }
            Err((ipc::IpcError::QueueFull, mut msg)) if !nonblock => {
                // Roll back moved cap before blocking/rescheduling.
                give_back(&mut msg);
                if typed.deadline_ns != 0 && ipc.timer.now() >= typed.deadline_ns {
                    return Err(Error::Ipc(ipc::IpcError::TimedOut));
                }
                ipc.prepare(BlockReason::IpcSend { endpoint, deadline_ns: typed.deadline_ns });
                // IMPORTANT: if the endpoint is gone, do not block (would deadlock forever).
                if let Err(e) = ipc.router.register_send_waiter(endpoint, me) {
                    ipc.abort();
                    return Err(e.into());
                }
                // Room may have come between the refusal and the registration: look again
                // before the commit (a receive wakes a parked sender only once it is listed).
                if !ipc.router.writable(endpoint) {
                    match ipc.sleep() {
                        Slept::Blocked => return Ok(Outcome::Blocked(Waits::Send(endpoint))),
                        Slept::Order => {
                            let _ = ipc.router.remove_send_waiter(endpoint, me);
                            return Ok(Outcome::Locked);
                        }
                        Slept::Woken => {}
                    }
                } else {
                    ipc.abort();
                }
                let _ = ipc.router.remove_send_waiter(endpoint, me);
                payload = Some(core::mem::take(&mut msg.payload));
            }
            Err((e, mut msg)) => {
                // Roll back the moved cap on any error so the caller does not lose it.
                give_back(&mut msg);
                #[cfg(feature = "ipc_trace_ring")]
                {
                    crate::ipc::trace::record_send(
                        me,
                        typed.slot.0 as u16,
                        endpoint,
                        msg.header.flags,
                        msg.payload.len() as u16,
                        Some(e),
                    );
                    if e == ipc::IpcError::NoSuchEndpoint {
                        crate::ipc::trace::dump_uart_send_nosuch(endpoint);
                    }
                }
                #[cfg(feature = "debug_uart")]
                if e == ipc::IpcError::NoSuchEndpoint {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = writeln!(
                        u,
                        "IPC-SEND nosuch ep=0x{:x} flags=0x{:x} capmove={}",
                        endpoint, msg.header.flags, cap_move
                    );
                }
                return Err(e.into());
            }
        }
    }
}

/// The table's entry of the v1 receive: the caller's pages committed, then the body.
pub(super) fn sys_ipc_recv_v1(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    IpcRecvV1ArgsTyped::decode(args)?.check(ctx)?;
    let out = recv_v1(&ctx.ipc()?, args);
    locked(ctx, out)
}

/// `ipc_recv_v1(slot, hdr_out, payload_out, payload_out_max, sys_flags, deadline_ns)`.
pub(super) fn recv_v1(ipc: &Ipc<'_>, args: &Args) -> IpcResult {
    let typed = IpcRecvV1ArgsTyped::decode(args)?;
    if (typed.sys_flags & !(IPC_SYS_NONBLOCK | IPC_SYS_TRUNCATE | IPC_SYS_EOF)) != 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let source = ipc.caps().pin_endpoint(typed.slot.0, Rights::RECV)?;
    let endpoint = source.id();
    // The waitset EOF latch (P7-d) is consumed when the receiver OBSERVES an empty queue, never
    // by dequeuing a frame (TASK-0054C P2-b): a peer that wrote and died leaves its frames AND
    // the latch — clearing on the frame's receive left a waitset blind to the death.
    let nonblock = (typed.sys_flags & IPC_SYS_NONBLOCK) != 0;
    let eof_opt = (typed.sys_flags & IPC_SYS_EOF) != 0;
    if !nonblock && typed.deadline_ns != 0 {
        crate::trap::arm_wakeup(ipc.timer, typed.deadline_ns);
    }
    let msg = match take(ipc, endpoint, nonblock, typed.deadline_ns, Some(eof_opt))? {
        Taken::Message(msg) => msg,
        Taken::Ended(outcome) => return Ok(outcome),
    };
    #[cfg(feature = "ipc_trace_ring")]
    crate::ipc::trace::record_recv(
        ipc.me.as_raw(),
        typed.slot.0 as u16,
        endpoint,
        msg.header.flags,
        msg.payload.len() as u16,
        None,
    );
    let out = Out {
        header_ptr: typed.header_out_ptr,
        payload_ptr: typed.payload_out_ptr,
        payload_max: typed.payload_out_max,
        sender_ptr: None,
        truncate: (typed.sys_flags & IPC_SYS_TRUNCATE) != 0,
    };
    deliver(ipc, endpoint, msg, &out)
}
