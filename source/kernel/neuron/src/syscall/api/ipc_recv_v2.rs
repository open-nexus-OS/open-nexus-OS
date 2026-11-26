// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the descriptor-based IPC recv-v2 syscall — like recv-v1 but also
//! returns the sender's kernel-derived service identity, via a versioned
//! descriptor that dodges the a0-a5 register limit. Split out of `ipc_msg.rs`
//! (RFC-0079, module-size ratchet). One body over `ipc_ctx::Ipc` (TASK-0330 P2,
//! ADR-0102): the trap's entry runs it before the kernel lock, the table's entry
//! under it after committing the pages the descriptor names.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker gates (recv-v2 sender-id probes)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::ipc_ctx::{deliver, locked, take, Ipc, IpcResult, Out, Outcome, Taken};
use super::*;

// IPC recv v2: descriptor-based syscall to return additional sender identity metadata without
// being limited by a0-a5 register count.
//
// Descriptor layout is versioned to keep the ABI extensible.
pub(super) const IPC_RECV_V2_MAGIC: u32 = 0x4E58_4932; // 'N''X''I''2'
pub(super) const IPC_RECV_V2_VERSION: u32 = 1;

/// The descriptor's size in the caller's memory: magic, version, slot, pad (4 × u32), the four
/// out words (4 × u64), flags and pad (2 × u32), the deadline (u64).
const DESC_BYTES: usize = 64;

/// What a receive descriptor names, decoded and bounded.
struct Desc {
    slot: usize,
    out: Out,
    nonblock: bool,
    deadline_ns: u64,
}

impl Desc {
    fn decode(raw: &[u8; DESC_BYTES]) -> Result<Self, Error> {
        let magic = read_u32_le(raw, 0)?;
        let version = read_u32_le(raw, 4)?;
        if magic != IPC_RECV_V2_MAGIC || version != IPC_RECV_V2_VERSION {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        let payload_max = read_u64_le(raw, 32)? as usize;
        let sys_flags = read_u32_le(raw, 48)? as usize;
        // EINVAL, not E2BIG: an out-buffer larger than any possible message is a caller bug,
        // not an oversize message (TASK-0054C P3b — E2BIG is the SEND-side answer).
        if payload_max > crate::ipc::IPC_PAYLOAD_MAX
            || (sys_flags & !(IPC_SYS_NONBLOCK | IPC_SYS_TRUNCATE)) != 0
        {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(Self {
            slot: read_u32_le(raw, 8)? as usize,
            out: Out {
                header_ptr: read_u64_le(raw, 16)? as usize,
                payload_ptr: read_u64_le(raw, 24)? as usize,
                payload_max,
                sender_ptr: Some(read_u64_le(raw, 40)? as usize),
                truncate: (sys_flags & IPC_SYS_TRUNCATE) != 0,
            },
            nonblock: (sys_flags & IPC_SYS_NONBLOCK) != 0,
            deadline_ns: read_u64_le(raw, 56)?,
        })
    }
}

/// Commits every page a receive through the descriptor at `desc_ptr` touches (ADR-0077): the
/// descriptor itself and each out-buffer it names — a lazy page before the receive can park.
pub(super) fn fill_recv_v2(ctx: &mut Context<'_>, desc_ptr: usize) -> Result<(), Error> {
    ensure_user_range(ctx, desc_ptr, DESC_BYTES, UserAccess::Read)?;
    let mut raw = [0u8; DESC_BYTES];
    if !ctx.ipc()?.read(desc_ptr, &mut raw) {
        return Err(AddressSpaceError::BadUserMemory.into());
    }
    let out = Desc::decode(&raw)?.out;
    ensure_user_range(ctx, out.header_ptr, 16, UserAccess::Write)?;
    if out.payload_max != 0 {
        ensure_user_range(ctx, out.payload_ptr, out.payload_max, UserAccess::Write)?;
    }
    ensure_user_range(ctx, out.sender_ptr.unwrap_or(0), 8, UserAccess::Write)
}

/// The table's entry: the caller's pages committed, then the body.
pub(super) fn sys_ipc_recv_v2(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    fill_recv_v2(ctx, args.get(0))?;
    let out = recv_v2(&ctx.ipc()?, args.get(0));
    locked(ctx, out)
}

/// `ipc_recv_v2(desc_ptr)`.
pub(super) fn recv_v2(ipc: &Ipc<'_>, desc_ptr: usize) -> IpcResult {
    let mut raw = [0u8; DESC_BYTES];
    if !ipc.read(desc_ptr, &mut raw) {
        return Ok(Outcome::Locked);
    }
    let desc = Desc::decode(&raw)?;
    let source = ipc.caps().pin_endpoint(desc.slot, Rights::RECV)?;
    let endpoint = source.id();
    // The waitset EOF latch is consumed when the receiver OBSERVES an empty queue, never by
    // dequeuing a frame (TASK-0054C P2-b; see the v1 receive).
    if !desc.nonblock && desc.deadline_ns != 0 {
        crate::trap::arm_wakeup(ipc.timer, desc.deadline_ns);
    }
    match take(ipc, endpoint, desc.nonblock, desc.deadline_ns, None)? {
        Taken::Message(msg) => deliver(ipc, endpoint, msg, &desc.out),
        Taken::Ended(outcome) => Ok(outcome),
    }
}
