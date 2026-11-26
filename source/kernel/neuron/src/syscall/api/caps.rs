// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Capability-management syscalls split out of the former
//! single-file api.rs: endpoint create/close (factory-gated v2/for),
//! sys_cap_close/clone/query and sys_cap_transfer(_to) incl. the MANAGE /
//! EndpointFactory transfer whitelists (RFC-0005 Phase 2 hardening).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: neuron host tests + QEMU marker gates (just test-os / ci-os-smp)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::*;

// Typed decoders for the Decode→Check→Execute syscall discipline

#[derive(Copy, Clone)]
pub(super) struct CapTransferArgsTyped {
    child: task::Pid,
    parent_slot: SlotIndex,
    rights_bits: u32,
}

impl CapTransferArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            child: task::Pid::from_raw(args.get(0) as u32),
            parent_slot: SlotIndex::decode(args.get(1)),
            rights_bits: args.get(2) as u32,
        })
    }
    #[inline]
    fn check(&self) -> Result<Rights, Error> {
        Rights::from_bits(self.rights_bits)
            .ok_or(Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied)))
    }
}

/// `cap_transfer_to`'s fifth argument (RFC-0112, amendment 2026-11-02): the target slot may be
/// empty or hold a capability to a DEAD endpoint, which is dropped and replaced — a restarted
/// server's route re-pinned into its clients' declared slots. A live capability, or one of any
/// other kind, is never replaced.
pub const CAP_TRANSFER_REPLACE_DEAD: usize = 1;

#[derive(Copy, Clone)]
pub(super) struct CapTransferToArgsTyped {
    child: task::Pid,
    parent_slot: SlotIndex,
    rights_bits: u32,
    child_slot: SlotIndex,
    flags: usize,
}

impl CapTransferToArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            child: task::Pid::from_raw(args.get(0) as u32),
            parent_slot: SlotIndex::decode(args.get(1)),
            rights_bits: args.get(2) as u32,
            child_slot: SlotIndex::decode(args.get(3)),
            flags: args.get(4),
        })
    }
    #[inline]
    fn check(&self) -> Result<Rights, Error> {
        if self.flags & !CAP_TRANSFER_REPLACE_DEAD != 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Rights::from_bits(self.rights_bits)
            .ok_or(Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied)))
    }
}

/// RFC-0112 (amendment 2026-11-02): a capability goes only into the caller's own table or into
/// one of its direct children — the parent that wires a child (init → its services, execd → its
/// apps, a work pool → its threads). Into a stranger it would take an empty slot the stranger's
/// parent was about to pin a route into: a forged route.
fn check_transfer_target(
    tasks: &task::TaskTable,
    caller: task::Pid,
    child: task::Pid,
) -> Result<(), Error> {
    let target = tasks.task(child).ok_or(Error::Transfer(task::TransferError::InvalidChild))?;
    let parent = target.parent().map(task::Pid::as_raw);
    if !crate::cap_rules::transfer_target_allowed(caller.as_raw(), child.as_raw(), parent) {
        return Err(Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied)));
    }
    Ok(())
}

/// REPLACE_DEAD's check on the target slot: empty, or a capability to a dead endpoint — which
/// is taken out (dropped with its header kept, to be judged after the transfer). Anything else
/// is `EPERM`.
fn take_dead_endpoint(
    tasks: &mut task::TaskTable,
    router: &ipc::Router,
    child: task::Pid,
    slot: usize,
) -> Result<Option<(ipc::EndpointId, crate::cap::Dropped)>, Error> {
    use crate::cap_rules::{replace_dead_verdict, ReplaceVerdict, SlotHolds};
    let cap_err = |e| Error::Transfer(task::TransferError::Capability(e));
    let mut caps =
        tasks.caps_of_mut(child).ok_or(Error::Transfer(task::TransferError::InvalidChild))?;
    let held = caps.peek(slot).map_err(cap_err)?;
    let holds = match held {
        None => SlotHolds::Empty,
        Some(Capability { kind: CapabilityKind::Endpoint(id), .. }) => {
            SlotHolds::Endpoint { alive: router.endpoint_alive(id) }
        }
        Some(_) => SlotHolds::Other,
    };
    match (replace_dead_verdict(holds), held) {
        (ReplaceVerdict::Pin, _) => Ok(None),
        (
            ReplaceVerdict::ReplaceDead,
            Some(Capability { kind: CapabilityKind::Endpoint(id), .. }),
        ) => {
            let dropped = caps.take_held(slot).map_err(cap_err)?.dropped();
            Ok(Some((id, dropped)))
        }
        _ => Err(cap_err(CapError::PermissionDenied)),
    }
}

pub(super) fn sys_ipc_endpoint_create(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    // Deprecated ABI: keep deterministic failure (use v2 with endpoint-factory cap).
    let _ = (ctx, args);
    Err(Error::Capability(CapError::PermissionDenied))
}

/// The caller holds the endpoint factory with `MANAGE` in `slot`. One look at its table: the
/// table's lock (rank 2) is gone before the router takes its registry's (rank 1).
fn factory_authority(ctx: &mut Context<'_>, slot: usize) -> SysResult<()> {
    let cap = ctx.tasks.current_caps_mut().get(slot)?;
    if cap.kind != CapabilityKind::EndpointFactory || !cap.rights.contains(Rights::MANAGE) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    Ok(())
}

pub(super) fn sys_ipc_endpoint_create_v2(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let factory_slot = args.get(0);
    let depth = args.get(1);
    if depth == 0 || depth > 256 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let current = ctx.tasks.current_pid();
    factory_authority(ctx, factory_slot)?;
    let id = super::ipc_reclaim::create_endpoint(ctx, depth, Some(current.as_raw()))?;
    let ep_cap = ctx.router.mint(id, Rights::SEND | Rights::RECV | Rights::MANAGE)?;
    let slot = ctx.tasks.current_caps_mut().allocate(ep_cap)?;
    Ok(slot)
}

pub(super) fn sys_ipc_endpoint_create_for(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let factory_slot = args.get(0);
    let owner_pid = task::Pid::from_raw(args.get(1) as u32);
    let depth = args.get(2);
    if depth == 0 || depth > 256 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }

    // Validate factory authority in the current task.
    let current = ctx.tasks.current_pid();
    factory_authority(ctx, factory_slot)?;

    // Validate the target owner exists.
    if ctx.tasks.task(owner_pid).is_none() {
        return Err(Error::Capability(CapError::PermissionDenied));
    }

    // Phase-2 hardening (authority tightening):
    // Even with an EndpointFactory, a task may only create endpoints owned by itself or by one of
    // its direct children. This prevents a compromised factory-holder from minting endpoints on
    // behalf of unrelated PIDs.
    if owner_pid != current {
        let parent = ctx.tasks.task(owner_pid).and_then(|t| t.parent());
        if parent != Some(current) {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
    }

    let id = super::ipc_reclaim::create_endpoint(ctx, depth, Some(owner_pid.as_raw()))?;
    #[cfg(feature = "ipc_trace_ring")]
    {
        crate::ipc::trace::record_ep_create(
            ctx.tasks.current_pid().as_raw(),
            id,
            depth as u16,
            owner_pid.as_raw() as u16,
        );
    }
    let ep_cap = ctx.router.mint(id, Rights::SEND | Rights::RECV | Rights::MANAGE)?;
    let slot = ctx.tasks.current_caps_mut().allocate(ep_cap)?;
    Ok(slot)
}
/// `SYSCALL_CAP_CLOSE` (20): drops the capability in `slot` and judges what that left of its
/// object — one body on either path, under no kernel lock (TASK-0330 P4). The capability is
/// taken out in one hold of the caller's table: a thread of the same process closing or filling
/// the slot beside this one cannot change what the judgement is about. Each object is then
/// judged behind its own locks. Local drop only: closing an endpoint for every holder is
/// `sys_ipc_endpoint_close` (`Rights::MANAGE`).
pub(super) fn cap_close(ipc: &ipc_ctx::Ipc<'_>, args: &Args) -> ipc_ctx::IpcResult {
    let held = ipc.caps().take_held(args.get(0))?;
    match held.cap().kind {
        CapabilityKind::Endpoint(id) => {
            // Counted off, its header kept for the judgement (the id stays the endpoint's):
            // one reading, three verdicts — RFC-0079 last-sender EOF for the receivers, the
            // senders' `PeerClosed`, RFC-0100 §1: an endpoint dies with its last capability.
            let _pin = held.dropped();
            let _ =
                super::eof_scan::judge_dropped(ipc.router, ipc.waitsets, ipc.timers, id, "close");
            // An endpoint that died of it took what its queue carried (RFC-0112).
            super::eof_scan::release_in_flight(ipc.router, ipc.waitsets, ipc.timers, "close");
        }
        CapabilityKind::Timer(id) => {
            drop(held);
            let _ = ipc.timers.free(crate::timer::TimerId(id));
        }
        CapabilityKind::Waitset(id) => {
            drop(held);
            let _ = ipc.waitsets.free(crate::waitset::WaitsetId(id));
        }
        CapabilityKind::Fence(id) => {
            drop(held);
            // Fence caps are transferable (workpool workers hold the parent's fences): only the
            // creator's close frees the kernel object; a non-owner close drops just the slot.
            // A fence's id is never another fence's (`sync::slot_table`): the look and the free
            // need no common hold.
            let fence_id = crate::fence::FenceId(id);
            if ipc.fences.owned_by(fence_id, ipc.me.as_raw()) {
                let _ = ipc.fences.free(fence_id);
            }
        }
        CapabilityKind::Vmo { id, .. } | CapabilityKind::VmoRo { id, .. } => {
            // Counted off first, then judged: RFC-0100 §1 — a VMO dies with its last
            // capability, when no address space maps it.
            drop(held);
            crate::mm::reclaim::orphans(&[id], "close");
        }
        _ => drop(held),
    }
    Ok(ipc_ctx::Outcome::Done(0))
}

pub(super) fn sys_cap_close(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let out = cap_close(&ctx.ipc()?, args);
    ipc_ctx::locked(ctx, out)
}

/// A second capability to what `slot` holds, in a slot of the same table — one hold of the
/// process's lock, on either path.
fn clone_in(mut caps: task::Caps, slot: usize) -> Result<usize, CapError> {
    let cap = caps.get(slot)?;
    // Security floor: EndpointFactory must not be duplicable.
    if cap.kind == CapabilityKind::EndpointFactory {
        return Err(CapError::PermissionDenied);
    }
    // A second capability of the same kind with the same rights, counted at its object.
    let clone = caps.dup(slot, cap.rights)?;
    caps.allocate(clone)
}

/// `SYSCALL_CAP_CLONE` (24) before the kernel lock (TASK-0330 P4).
pub(super) fn cap_clone(ipc: &ipc_ctx::Ipc<'_>, args: &Args) -> ipc_ctx::IpcResult {
    Ok(ipc_ctx::Outcome::Done(clone_in(ipc.caps(), args.get(0))?))
}

pub(super) fn sys_cap_clone(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    Ok(clone_in(ctx.tasks.current_caps_mut(), args.get(0))?)
}

pub(super) fn sys_ipc_endpoint_close(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    // Dropped with its header kept: the id is the endpoint's from the drop to the forget.
    let dropped = ctx.tasks.current_caps_mut().take_held(slot)?.dropped();
    let CapabilityKind::Endpoint(id) = dropped.cap.kind else {
        return Err(Error::Capability(CapError::InvalidSlot));
    };
    if !dropped.cap.rights.contains(Rights::MANAGE) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    #[cfg(feature = "ipc_trace_ring")]
    {
        crate::ipc::trace::record_ep_close(ctx.tasks.current_pid().as_raw(), id);
    }
    let waiters = ctx.router.close_endpoint(id)?;
    super::ipc_reclaim::forget_dead(ctx.waitsets, ctx.hart_timers, id);
    for pid in waiters {
        observe_wake_outcome(ctx.tasks.wake(task::Pid::from_raw(pid), ctx.scheduler));
    }
    // What its queue carried is dropped like a close of each (RFC-0112).
    super::eof_scan::release_in_flight(ctx.router, ctx.waitsets, ctx.hart_timers, "close");
    drop(dropped);
    Ok(0)
}

pub(super) fn sys_cap_query(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = SlotIndex::decode(args.get(0));
    let out_ptr = args.get(1);
    // out layout (LE):
    // - u32 kind_tag (1=vmo, 2=device_mmio, 3=vmo_ro)
    // - u32 irq (device_mmio: the PLIC line from the tree, 0 = none; else 0)
    // - u64 base (device_mmio: the register window; 0 for a VMO — its
    //   physical runs leave the kernel only through `vmo_runs`, RFC-0098 C4)
    // - u64 len
    // - u32 flags (device_mmio: bit 0 = the device does not snoop the caches)
    // - u32 cache_block (device_mmio: the harts' Zicbom block size, 0 = none —
    //   RFC-0098 C4; what a driver of a non-coherent device maintains in)
    const OUT_LEN: usize = 32;
    ensure_user_range(ctx, out_ptr, OUT_LEN, UserAccess::Write)?;

    // Capability gate: require MAP rights to introspect address-bearing caps.
    let cap = ctx.tasks.current_caps_mut().derive(slot.0, Rights::MAP)?;
    let (mut flags, mut cache_block) = (0u32, 0u32);
    let (kind_tag, irq, base, len) = match cap.kind {
        CapabilityKind::Vmo { len, .. } => (1u32, 0u32, 0u64, len as u64),
        CapabilityKind::DeviceMmio { base, len, irq, dma_noncoherent, .. } => {
            flags = u32::from(dma_noncoherent);
            cache_block = crate::hal::platform::cbom_block() as u32;
            (2u32, irq, base as u64, len as u64)
        }
        CapabilityKind::VmoRo { len, .. } => (3u32, 0u32, 0u64, len as u64),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };

    let mut out = [0u8; OUT_LEN];
    out[0..4].copy_from_slice(&kind_tag.to_le_bytes());
    out[4..8].copy_from_slice(&irq.to_le_bytes());
    out[8..16].copy_from_slice(&base.to_le_bytes());
    out[16..24].copy_from_slice(&len.to_le_bytes());
    out[24..28].copy_from_slice(&flags.to_le_bytes());
    out[28..32].copy_from_slice(&cache_block.to_le_bytes());
    unsafe {
        core::ptr::copy_nonoverlapping(out.as_ptr(), out_ptr as *mut u8, OUT_LEN);
    }
    Ok(0)
}

pub(super) fn sys_cap_transfer(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = CapTransferArgsTyped::decode(args)?;
    let rights = typed.check()?;
    let parent = ctx.tasks.current_pid();
    #[cfg(feature = "ipc_trace_ring")]
    {
        if let Ok(parent_caps) =
            ctx.tasks.caps_of(parent).ok_or(Error::Transfer(task::TransferError::InvalidParent))
        {
            if let Ok(base) = parent_caps.get(typed.parent_slot.0) {
                if let CapabilityKind::Endpoint(id) = base.kind {
                    crate::ipc::trace::record_cap_xfer(
                        parent.as_raw(),
                        typed.child.as_raw(),
                        id,
                        rights.bits() as u16,
                    );
                }
            }
        }
    }
    check_manage_travels(ctx.tasks, parent, typed.parent_slot.0, rights)?;

    // Phase-2 hardening (factory distribution): EndpointFactory is not a general transferable cap.
    // Until policyd-gated distribution exists, only bootstrap (PID 0) may transfer it into init-lite (PID 1).
    // This keeps endpoint-mint authority centralized in init-lite during bring-up.
    if let Ok(parent_caps) =
        ctx.tasks.caps_of(parent).ok_or(Error::Transfer(task::TransferError::InvalidParent))
    {
        // (This block is structured as "check then act" to keep denial deterministic.)
        if let Ok(base) = parent_caps.get(typed.parent_slot.0) {
            if base.kind == CapabilityKind::EndpointFactory
                && !(parent == task::Pid::KERNEL && typed.child == task::Pid::from_raw(1))
            {
                return Err(Error::Transfer(task::TransferError::Capability(
                    CapError::PermissionDenied,
                )));
            }
        }
    }
    check_transfer_target(ctx.tasks, parent, typed.child)?;
    let slot = ctx.tasks.transfer_cap(parent, typed.child, typed.parent_slot.0, rights)?;
    Ok(slot)
}

/// RFC-0005 phase 2 (hardening): a transfer carries `MANAGE` only for the kinds whose `MANAGE`
/// is the holder's to hand on (`cap_rules::manage_travels`) — the endpoint factory (init holds
/// the authority to make endpoints), a fence (a work pool hands its fences to its own threads)
/// and a time right (RFC-0120: a parent hands a child the right it derived for it). Never an
/// endpoint's.
fn check_manage_travels(
    tasks: &task::TaskTable,
    parent: task::Pid,
    slot: usize,
    rights: Rights,
) -> Result<(), Error> {
    use crate::cap_rules::{manage_travels, Carries};
    if !rights.contains(Rights::MANAGE) {
        return Ok(());
    }
    let caps = tasks.caps_of(parent).ok_or(Error::Transfer(task::TransferError::InvalidParent))?;
    let base = caps.get(slot).map_err(|e| Error::Transfer(task::TransferError::Capability(e)))?;
    let carries = match base.kind {
        CapabilityKind::EndpointFactory => Carries::EndpointFactory,
        CapabilityKind::Fence(_) => Carries::Fence,
        CapabilityKind::TimeRight(_) => Carries::TimeRight,
        _ => Carries::Other,
    };
    if !manage_travels(carries) {
        return Err(Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied)));
    }
    Ok(())
}

pub(super) fn sys_cap_transfer_to(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = CapTransferToArgsTyped::decode(args)?;
    let rights = typed.check()?;
    let parent = ctx.tasks.current_pid();
    check_manage_travels(ctx.tasks, parent, typed.parent_slot.0, rights)?;

    // Phase-2 hardening (factory distribution): EndpointFactory is not a general transferable cap.
    // Until policyd-gated distribution exists, only bootstrap (PID 0) may transfer it into init-lite (PID 1).
    // This keeps endpoint-mint authority centralized in init-lite during bring-up.
    if let Ok(parent_caps) =
        ctx.tasks.caps_of(parent).ok_or(Error::Transfer(task::TransferError::InvalidParent))
    {
        // (This block is structured as "check then act" to keep denial deterministic.)
        if let Ok(base) = parent_caps.get(typed.parent_slot.0) {
            if base.kind == CapabilityKind::EndpointFactory
                && !(parent == task::Pid::KERNEL && typed.child == task::Pid::from_raw(1))
            {
                return Err(Error::Transfer(task::TransferError::Capability(
                    CapError::PermissionDenied,
                )));
            }
        }
    }
    check_transfer_target(ctx.tasks, parent, typed.child)?;
    let replaced = if typed.flags & CAP_TRANSFER_REPLACE_DEAD != 0 {
        // The capability the slot will hold is checked before the dead one goes.
        ctx.tasks
            .caps_of(parent)
            .ok_or(Error::Transfer(task::TransferError::InvalidParent))?
            .derive(typed.parent_slot.0, rights)
            .map_err(|e| Error::Transfer(task::TransferError::Capability(e)))?;
        take_dead_endpoint(ctx.tasks, ctx.router, typed.child, typed.child_slot.0)?
    } else {
        None
    };
    ctx.tasks.transfer_cap_to_slot(
        parent,
        typed.child,
        typed.parent_slot.0,
        rights,
        typed.child_slot.0,
    )?;
    if let Some((id, _pin)) = replaced {
        // The one drop path, though a dead endpoint's counts change nothing.
        let _ = super::eof_scan::judge_dropped(
            ctx.router,
            ctx.waitsets,
            ctx.hart_timers,
            id,
            "replace",
        );
    }
    Ok(typed.child_slot.0)
}
