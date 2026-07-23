// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the descriptor-based IPC recv-v2 syscall — like recv-v1 but also
//! returns the sender's kernel-derived service identity, via a versioned
//! descriptor that dodges the a0-a5 register limit. Split out of `ipc_msg.rs`
//! (RFC-0079, module-size ratchet). Shares the typed decoders + helpers of the
//! parent via `use super::*`.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker gates (recv-v2 sender-id probes)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::*;

// IPC recv v2: descriptor-based syscall to return additional sender identity metadata without
// being limited by a0-a5 register count.
//
// Descriptor layout is versioned to keep the ABI extensible.
pub(super) const IPC_RECV_V2_MAGIC: u32 = 0x4E58_4932; // 'N''X''I''2'
pub(super) const IPC_RECV_V2_VERSION: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct IpcRecvV2Desc {
    magic: u32,
    version: u32,
    slot: u32,
    _pad0: u32,
    header_out_ptr: u64,
    payload_out_ptr: u64,
    payload_out_max: u64,
    sender_service_id_out_ptr: u64,
    sys_flags: u32,
    _pad1: u32,
    deadline_ns: u64,
}

pub(super) fn sys_ipc_recv_v2(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let desc_ptr = args.get(0);
    // Defensive: require the descriptor itself to be a valid user slice.
    ensure_user_slice(desc_ptr, core::mem::size_of::<IpcRecvV2Desc>())?;
    let mut raw = [0u8; core::mem::size_of::<IpcRecvV2Desc>()];
    unsafe {
        core::ptr::copy_nonoverlapping(desc_ptr as *const u8, raw.as_mut_ptr(), raw.len());
    }

    let magic = read_u32_le(&raw, 0)?;
    let version = read_u32_le(&raw, 4)?;
    if magic != IPC_RECV_V2_MAGIC || version != IPC_RECV_V2_VERSION {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let slot = read_u32_le(&raw, 8)? as u32;
    let header_out_ptr = read_u64_le(&raw, 16)? as usize;
    let payload_out_ptr = read_u64_le(&raw, 24)? as usize;
    let payload_out_max = read_u64_le(&raw, 32)? as usize;
    let sender_service_id_out_ptr = read_u64_le(&raw, 40)? as usize;
    let sys_flags = read_u32_le(&raw, 48)? as usize;
    let deadline_ns = read_u64_le(&raw, 56)?;

    // Validate pointers up-front (RFC-0004 style provenance).
    ensure_user_slice(header_out_ptr, 16)?;
    const MAX_FRAME_BYTES: usize = 8 * 1024;
    if payload_out_max > MAX_FRAME_BYTES {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    if payload_out_max != 0 {
        ensure_user_slice(payload_out_ptr, payload_out_max)?;
    }
    ensure_user_slice(sender_service_id_out_ptr, 8)?;

    if (sys_flags & !(IPC_SYS_NONBLOCK | IPC_SYS_TRUNCATE)) != 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }

    // Derive endpoint.
    let endpoint =
        ctx.tasks.current_caps_mut().derive_endpoint_ref(slot as usize, Rights::RECV)?.endpoint();

    let truncate = (sys_flags & IPC_SYS_TRUNCATE) != 0;
    let nonblock = (sys_flags & IPC_SYS_NONBLOCK) != 0;
    if !nonblock && deadline_ns != 0 {
        crate::trap::arm_wakeup(ctx.timer, deadline_ns);
    }

    let mut msg = loop {
        match ctx.router.recv(endpoint) {
            Ok(msg) => {
                if let Ok(Some(waiter)) = ctx.router.pop_send_waiter(endpoint) {
                    observe_wake_outcome(
                        ctx.tasks.wake(task::Pid::from_raw(waiter), ctx.scheduler),
                    );
                }
                break msg;
            }
            Err(ipc::IpcError::QueueEmpty) if !nonblock => {
                if deadline_ns != 0 && ctx.timer.now() >= deadline_ns {
                    return Err(Error::Ipc(ipc::IpcError::TimedOut));
                }
                let cur = ctx.tasks.current_pid();
                ctx.router.register_recv_waiter(endpoint, cur.as_raw())?;
                // Avoid missed-wakeup: re-check after registering.
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
                    Err(ipc::IpcError::QueueEmpty) => {}
                    Err(e) => {
                        let _ = ctx.router.remove_recv_waiter(endpoint, cur.as_raw());
                        return Err(e.into());
                    }
                }
                ctx.tasks
                    .block_current(BlockReason::IpcRecv { endpoint, deadline_ns }, ctx.scheduler);
                wake_expired_blocked(ctx);
                if let Some(next) = ctx.scheduler.schedule_next() {
                    ctx.tasks.set_current(next);
                    return Err(Error::Reschedule);
                }
                let _ = ctx.router.remove_recv_waiter(endpoint, cur.as_raw());
                observe_wake_outcome(ctx.tasks.wake(cur, ctx.scheduler));
                return Err(Error::Reschedule);
            }
            Err(e) => return Err(e.into()),
        }
    };

    // CAP_MOVE allocation (same semantics as v1).
    if let Some(mut cap) = msg.moved_cap.take() {
        if msg.capmove_expected_ep != 0 {
            if let CapabilityKind::Endpoint(id) = cap.kind {
                if id != msg.capmove_expected_ep {
                    cap.kind = CapabilityKind::Endpoint(msg.capmove_expected_ep);
                }
            }
        }
        match ctx.tasks.current_caps_mut().allocate(cap) {
            Ok(slot) => {
                msg.header.src = slot as u32;
            }
            Err(_) => {
                msg.moved_cap = Some(cap);
                let _ = ctx.router.requeue_front(endpoint, msg);
                return Err(Error::Ipc(ipc::IpcError::NoSpace));
            }
        }
    }

    // Copy-out header.
    let hdr = msg.header.to_le_bytes();
    unsafe {
        core::ptr::copy_nonoverlapping(hdr.as_ptr(), header_out_ptr as *mut u8, hdr.len());
    }

    // Copy-out sender service id (kernel-derived).
    let sid = msg.sender_service_id.to_le_bytes();
    unsafe {
        core::ptr::copy_nonoverlapping(
            sid.as_ptr(),
            sender_service_id_out_ptr as *mut u8,
            sid.len(),
        );
    }

    let total = msg.payload.len();
    if total == 0 || payload_out_max == 0 {
        ctx.last_message = Some(msg);
        return Ok(0);
    }
    if total > payload_out_max && !truncate {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let n = core::cmp::min(total, payload_out_max);
    unsafe {
        core::ptr::copy_nonoverlapping(msg.payload.as_ptr(), payload_out_ptr as *mut u8, n);
    }
    ctx.last_message = Some(msg);
    Ok(n)
}
