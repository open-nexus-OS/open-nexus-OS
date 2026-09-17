// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: IPC message-transfer syscalls split out of the former single-file
//! api.rs: legacy sys_send/sys_recv, sys_ipc_send_v1 (incl. the CAP_MOVE
//! take/rollback path), sys_ipc_recv_v1 and the descriptor-based
//! sys_ipc_recv_v2 (sender identity metadata), plus their typed decoders.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: neuron host tests + QEMU marker gates (just test-os / ci-os-smp)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

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

/// RFC-0079: true while a PEER (any task but the endpoint's owner) still holds a live SEND
/// cap to `endpoint` — the one scan every EOF path shares (`eof_scan`, TASK-0324 P7-b).
fn any_sender_cap(ctx: &Context<'_>, endpoint: ipc::EndpointId) -> bool {
    super::eof_scan::foreign_sender_remains(ctx.tasks, ctx.router, endpoint)
}

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

    #[inline]
    fn check(&self) -> Result<(), Error> {
        ensure_user_slice(self.header_ptr, 16)?;
        if self.payload_len > crate::ipc::IPC_PAYLOAD_MAX {
            // E2BIG, not EINVAL: "your message is too big" is not "you passed a bad pointer",
            // and userspace could not tell them apart before (TASK-0054C P3b, ADR-0054).
            return Err(Error::Ipc(ipc::IpcError::TooBig));
        }
        if self.payload_len != 0 {
            ensure_user_slice(self.payload_ptr, self.payload_len)?;
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

    #[inline]
    fn check(&self) -> Result<(), Error> {
        ensure_user_slice(self.header_out_ptr, 16)?;
        if self.payload_out_max != 0 {
            ensure_user_slice(self.payload_out_ptr, self.payload_out_max)?;
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

pub(super) fn sys_ipc_send_v1(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = IpcSendV1ArgsTyped::decode(args)?;
    typed.check()?;

    if (typed.sys_flags & !(IPC_SYS_NONBLOCK)) != 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let nonblock = (typed.sys_flags & IPC_SYS_NONBLOCK) != 0;

    let endpoint =
        ctx.tasks.current_caps_mut().derive_endpoint_ref(typed.slot.0, Rights::SEND)?.endpoint();

    let mut hdr_bytes = [0u8; 16];
    unsafe {
        core::ptr::copy_nonoverlapping(
            typed.header_ptr as *const u8,
            hdr_bytes.as_mut_ptr(),
            hdr_bytes.len(),
        );
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
    // message and the kernel heap is never touched (TASK-0054C P3b). `check()` above proved the
    // slice and the bound, which is exactly this call's safety contract.
    let payload = unsafe {
        ipc::Payload::copy_in_from_user(typed.payload_ptr as *const u8, typed.payload_len)
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
    let header = MessageHeader::new(
        0,
        ctx.tasks.current_pid().as_raw(),
        user_hdr.ty,
        user_hdr.flags,
        typed.payload_len as u32,
    );

    if !nonblock && typed.deadline_ns != 0 {
        crate::trap::arm_wakeup(ctx.timer, typed.deadline_ns);
    }
    loop {
        // If CAP_MOVE is set, take the cap for this attempt. If the attempt fails (QueueFull,
        // NoSuchEndpoint, etc.) we restore it before returning/rescheduling.
        let moved_cap = if let Some(slot) = cap_move_slot {
            let cap = ctx.tasks.current_caps_mut().take(slot)?;
            // Security floor: never allow moving MANAGE authority in-band.
            if cap.rights.contains(Rights::MANAGE) || cap.kind == CapabilityKind::EndpointFactory {
                let _ = ctx.tasks.current_caps_mut().set(slot, cap);
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            // Hardening: do not allow CAP_MOVE of dead/non-existent endpoints.
            if let CapabilityKind::Endpoint(id) = cap.kind {
                if !ctx.router.endpoint_alive(id) {
                    let _ = ctx.tasks.current_caps_mut().set(slot, cap);
                    return Err(Error::Ipc(ipc::IpcError::NoSuchEndpoint));
                }
                #[cfg(feature = "ipc_trace_ring")]
                {
                    crate::ipc::trace::record_capmove_send(
                        ctx.tasks.current_pid().as_raw(),
                        typed.slot.0 as u16,
                        slot as u16,
                        endpoint,
                        id,
                    );
                }
            }
            Some(cap)
        } else {
            None
        };
        // The payload MOVES in (TASK-0054C P3a). It was cloned here "in case the attempt
        // fails", but this loop has no `continue` and a failed send hands the whole `Message`
        // back via `Err((IpcError, Message))` — the clone was half of the 2.000 allocations.
        let mut msg = ipc::Message::new(header, payload, moved_cap);
        if cap_move {
            if let Some(cap) = msg.moved_cap {
                if let CapabilityKind::Endpoint(id) = cap.kind {
                    msg.capmove_expected_ep = id;
                }
            }
        }
        msg.sender_service_id = ctx.tasks.current_service_id();
        #[cfg(feature = "ipc_trace_ring")]
        {
            crate::ipc::trace::record_send(
                ctx.tasks.current_pid().as_raw(),
                typed.slot.0 as u16,
                endpoint,
                msg.header.flags,
                msg.payload.len() as u16,
                None,
            );
        }
        #[cfg(feature = "debug_uart")]
        {
            let p = msg.payload.as_slice();
            if p.len() >= 4 && p[0] == b'S' && p[1] == b'M' && p[2] == 1 && p[3] == 1 {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = writeln!(u, "IPC-SEND samgr reg ep=0x{:x}", endpoint);
            }
        }
        match ctx.router.send_returning_message(endpoint, msg) {
            Ok(()) => {
                crate::ipc_stats::record_send();
                // Wake one receiver blocked on this endpoint (if any).
                if let Ok(Some(waiter)) = ctx.router.pop_recv_waiter(endpoint) {
                    let pid = task::Pid::from_raw(waiter);
                    let here = crate::smp::cpu_current_id();
                    let cross = ctx.tasks.task(pid).is_some_and(|t| t.home_cpu() != here);
                    crate::ipc_stats::record_recv_wake(cross);
                    observe_wake_outcome(ctx.tasks.wake(pid, ctx.scheduler));
                }
                // Low-noise triage: dump trace ring once on the first "large CAP_MOVE" send.
                // This helps diagnose OTA stage hangs without relying on NoSuchEndpoint spam.
                #[cfg(feature = "ipc_trace_ring")]
                if cap_move && typed.payload_len > 1024 {
                    crate::ipc::trace::maybe_dump_capmove_big("capmove-big");
                }
                return Ok(typed.payload_len);
            }
            Err((ipc::IpcError::QueueFull, msg)) if !nonblock => {
                // Roll back moved cap before blocking/rescheduling.
                if let Some(slot) = cap_move_slot {
                    if let Some(cap) = msg.moved_cap {
                        let _ = ctx.tasks.current_caps_mut().set(slot, cap);
                    }
                }
                if typed.deadline_ns != 0 && ctx.timer.now() >= typed.deadline_ns {
                    return Err(Error::Ipc(ipc::IpcError::TimedOut));
                }
                let cur = ctx.tasks.current_pid();
                // IMPORTANT: if the endpoint is gone, do not block (would deadlock forever).
                ctx.router.register_send_waiter(endpoint, cur.as_raw())?;
                ctx.tasks.block_current(
                    BlockReason::IpcSend { endpoint, deadline_ns: typed.deadline_ns },
                    ctx.scheduler,
                );
                wake_expired_blocked(ctx);
                if let Some(next) = ctx.scheduler.schedule_next() {
                    ctx.tasks.set_current(next);
                    return Err(Error::Reschedule);
                }
                // Nothing runnable on this hart: park it (legacy self-wake pre-runtime).
                return Err(park_hart_or_self_wake(ctx, cur, |ctx| {
                    let _ = ctx.router.remove_send_waiter(endpoint, cur.as_raw());
                }));
            }
            Err((e, msg)) => {
                // Roll back the moved cap on any error so the caller does not lose it.
                if let Some(slot) = cap_move_slot {
                    if let Some(cap) = msg.moved_cap {
                        let _ = ctx.tasks.current_caps_mut().set(slot, cap);
                    }
                }
                #[cfg(feature = "ipc_trace_ring")]
                {
                    crate::ipc::trace::record_send(
                        ctx.tasks.current_pid().as_raw(),
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
                        endpoint,
                        msg.header.flags,
                        msg.moved_cap.is_some()
                    );
                }
                return Err(e.into());
            }
        }
    }
}

pub(super) fn sys_ipc_recv_v1(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = IpcRecvV1ArgsTyped::decode(args)?;
    typed.check()?;

    if (typed.sys_flags & !(IPC_SYS_NONBLOCK | IPC_SYS_TRUNCATE | IPC_SYS_EOF)) != 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }

    let endpoint =
        ctx.tasks.current_caps_mut().derive_endpoint_ref(typed.slot.0, Rights::RECV)?.endpoint();
    // The waitset EOF latch (P7-d) is consumed below when the receiver OBSERVES an empty queue,
    // never by dequeuing a frame (TASK-0054C P2-b): a peer that wrote and died leaves its frames
    // AND the latch — clearing on the frame's receive left a waitset blind to the death.
    let truncate = (typed.sys_flags & IPC_SYS_TRUNCATE) != 0;
    let nonblock = (typed.sys_flags & IPC_SYS_NONBLOCK) != 0;
    let eof_opt = (typed.sys_flags & IPC_SYS_EOF) != 0;
    if !nonblock && typed.deadline_ns != 0 {
        crate::trap::arm_wakeup(ctx.timer, typed.deadline_ns);
    }
    let mut msg = loop {
        match ctx.router.recv(endpoint) {
            Ok(msg) => {
                // Receiving frees queue capacity; wake one sender blocked on this endpoint (if any).
                if let Ok(Some(waiter)) = ctx.router.pop_send_waiter(endpoint) {
                    observe_wake_outcome(
                        ctx.tasks.wake(task::Pid::from_raw(waiter), ctx.scheduler),
                    );
                }
                break msg;
            }
            Err(ipc::IpcError::QueueEmpty) if !nonblock => {
                ctx.router.clear_eof_pending(endpoint);
                if typed.deadline_ns != 0 && ctx.timer.now() >= typed.deadline_ns {
                    return Err(Error::Ipc(ipc::IpcError::TimedOut));
                }
                let cur = ctx.tasks.current_pid();
                // IMPORTANT: if the endpoint is gone, do not block (would deadlock forever).
                ctx.router.register_recv_waiter(endpoint, cur.as_raw())?;
                // Avoid missed-wakeup: a sender can enqueue between our empty check and waiter
                // registration. Re-check once after registering; if a message is present, consume
                // it without blocking.
                match ctx.router.recv(endpoint) {
                    Ok(msg) => {
                        let _ = ctx.router.remove_recv_waiter(endpoint, cur.as_raw());
                        if let Ok(Some(waiter)) = ctx.router.pop_send_waiter(endpoint) {
                            observe_wake_outcome(
                                ctx.tasks.wake(task::Pid::from_raw(waiter), ctx.scheduler),
                            );
                        }
                        break msg;
                    }
                    Err(ipc::IpcError::QueueEmpty) => {
                        // Proceed to block below — or EOF if opted-in and the
                        // last sender is gone.
                    }
                    Err(e) => {
                        let _ = ctx.router.remove_recv_waiter(endpoint, cur.as_raw());
                        return Err(e.into());
                    }
                }
                // RFC-0079 last-sender EOF (opt-in): decide via the pure,
                // host-tested predicate. A live sender also latches the
                // endpoint (belt-and-suspenders with the send path).
                if eof_opt {
                    let any_sender = any_sender_cap(ctx, endpoint);
                    if any_sender {
                        ctx.router.mark_endpoint_had_sender(endpoint);
                    }
                    let had_sender = ctx.router.endpoint_had_sender(endpoint);
                    if crate::ipc_eof::should_disconnect(true, had_sender, any_sender) {
                        let _ = ctx.router.remove_recv_waiter(endpoint, cur.as_raw());
                        return Err(Error::Ipc(ipc::IpcError::PeerClosed));
                    }
                }
                ctx.tasks.block_current(
                    BlockReason::IpcRecv { endpoint, deadline_ns: typed.deadline_ns },
                    ctx.scheduler,
                );
                wake_expired_blocked(ctx);
                if let Some(next) = ctx.scheduler.schedule_next() {
                    ctx.tasks.set_current(next);
                    return Err(Error::Reschedule);
                }
                // Nothing runnable on this hart: park it (legacy self-wake pre-runtime).
                return Err(park_hart_or_self_wake(ctx, cur, |ctx| {
                    let _ = ctx.router.remove_recv_waiter(endpoint, cur.as_raw());
                }));
            }
            Err(e) => {
                if matches!(e, ipc::IpcError::QueueEmpty) {
                    ctx.router.clear_eof_pending(endpoint);
                }
                return Err(e.into());
            }
        }
    };
    #[cfg(feature = "ipc_trace_ring")]
    {
        crate::ipc::trace::record_recv(
            ctx.tasks.current_pid().as_raw(),
            typed.slot.0 as u16,
            endpoint,
            msg.header.flags,
            msg.payload.len() as u16,
            None,
        );
    }

    // If the message carries a moved capability, allocate it into the receiver now and write the
    // allocated slot into the returned header's `src` field.
    if let Some(mut cap) = msg.moved_cap.take() {
        // CAP_MOVE robustness: if the moved endpoint id is inconsistent with what the sender
        // observed, prefer the sender's value (kernel internal field).
        if msg.capmove_expected_ep != 0 {
            if let CapabilityKind::Endpoint(id) = cap.kind {
                if id != msg.capmove_expected_ep {
                    #[cfg(feature = "ipc_trace_ring")]
                    {
                        use core::fmt::Write as _;
                        let mut u = crate::uart::raw_writer();
                        let _ = writeln!(
                            u,
                            "IPC-CAPMOVE fix exp=0x{:x} got=0x{:x}",
                            msg.capmove_expected_ep, id
                        );
                    }
                    cap.kind = CapabilityKind::Endpoint(msg.capmove_expected_ep);
                }
            }
        }
        #[cfg(feature = "ipc_trace_ring")]
        let moved_ep_for_trace: u32 = match cap.kind {
            CapabilityKind::Endpoint(id) => id,
            _ => 0,
        };
        #[cfg(feature = "debug_uart")]
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let mut cap_info = 0usize;
            if let CapabilityKind::Endpoint(id) = cap.kind {
                cap_info = id as usize;
            }
            let _ = writeln!(u, "IPC-CAPMOVE recv ep=0x{:x} cap_ep=0x{:x}", endpoint, cap_info);
        }
        match ctx.tasks.current_caps_mut().allocate(cap) {
            Ok(slot) => {
                #[cfg(feature = "ipc_trace_ring")]
                {
                    crate::ipc::trace::record_capmove_alloc(
                        ctx.tasks.current_pid().as_raw(),
                        endpoint,
                        slot as u32,
                        moved_ep_for_trace,
                    );
                }
                #[cfg(feature = "debug_uart")]
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = writeln!(u, "IPC-CAPMOVE recv slot=0x{:x}", slot);
                }
                msg.header.src = slot as u32;
                // Low-noise triage: dump trace ring once on the first "large CAP_MOVE" receive
                // after the capability has been allocated (so we can correlate with the sender dump).
                #[cfg(feature = "ipc_trace_ring")]
                if msg.header.len > 1024 {
                    crate::ipc::trace::maybe_dump_capmove_big_recv("capmove-big-recv");
                }
            }
            Err(_) => {
                #[cfg(feature = "debug_uart")]
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = writeln!(u, "IPC-CAPMOVE recv nospace");
                }
                // Roll back: receiver cannot accept a moved cap right now (e.g. no free cap slots).
                // Re-queue the message and surface a stable syscall error (ENOSPC).
                msg.moved_cap = Some(cap);
                let _ = ctx.router.requeue_front(endpoint, msg);
                return Err(Error::Ipc(ipc::IpcError::NoSpace));
            }
        }
    }

    // Copy-out header (always).
    let hdr = msg.header.to_le_bytes();
    unsafe {
        core::ptr::copy_nonoverlapping(hdr.as_ptr(), typed.header_out_ptr as *mut u8, hdr.len());
    }

    let total = msg.payload.len();
    if total == 0 || typed.payload_out_max == 0 {
        ctx.last_message = Some(msg);
        return Ok(0);
    }

    if total > typed.payload_out_max && !truncate {
        return Err(AddressSpaceError::InvalidArgs.into());
    }

    let n = core::cmp::min(total, typed.payload_out_max);
    unsafe {
        core::ptr::copy_nonoverlapping(msg.payload.as_ptr(), typed.payload_out_ptr as *mut u8, n);
    }
    crate::ipc_stats::record_payload_copy(n);

    ctx.last_message = Some(msg);
    Ok(n)
}
