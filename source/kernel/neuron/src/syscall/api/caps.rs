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

// Typed decoders for seL4-style Decode→Check→Execute

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

#[derive(Copy, Clone)]
pub(super) struct CapTransferToArgsTyped {
    child: task::Pid,
    parent_slot: SlotIndex,
    rights_bits: u32,
    child_slot: SlotIndex,
}

impl CapTransferToArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            child: task::Pid::from_raw(args.get(0) as u32),
            parent_slot: SlotIndex::decode(args.get(1)),
            rights_bits: args.get(2) as u32,
            child_slot: SlotIndex::decode(args.get(3)),
        })
    }
    #[inline]
    fn check(&self) -> Result<Rights, Error> {
        Rights::from_bits(self.rights_bits)
            .ok_or(Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied)))
    }
}

pub(super) fn sys_ipc_endpoint_create(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    // Deprecated ABI: keep deterministic failure (use v2 with endpoint-factory cap).
    let _ = (ctx, args);
    Err(Error::Capability(CapError::PermissionDenied))
}

pub(super) fn sys_ipc_endpoint_create_v2(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let factory_slot = args.get(0);
    let depth = args.get(1);
    if depth == 0 || depth > 256 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let current = ctx.tasks.current_pid();
    let cap_table =
        ctx.tasks.caps_of(current).ok_or(Error::Capability(CapError::PermissionDenied))?;
    let cap = cap_table.get(factory_slot)?;
    if cap.kind != CapabilityKind::EndpointFactory || !cap.rights.contains(Rights::MANAGE) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let id = ctx.router.create_endpoint(depth, Some(current.as_raw()))?;
    let ep_cap = Capability {
        kind: CapabilityKind::Endpoint(id),
        rights: Rights::SEND | Rights::RECV | Rights::MANAGE,
    };
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
    let cap_table =
        ctx.tasks.caps_of(current).ok_or(Error::Capability(CapError::PermissionDenied))?;
    let cap = cap_table.get(factory_slot)?;
    if cap.kind != CapabilityKind::EndpointFactory || !cap.rights.contains(Rights::MANAGE) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }

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

    let id = ctx.router.create_endpoint(depth, Some(owner_pid.as_raw()))?;
    #[cfg(feature = "ipc_trace_ring")]
    {
        crate::ipc::trace::record_ep_create(
            ctx.tasks.current_pid().as_raw(),
            id,
            depth as u16,
            owner_pid.as_raw() as u16,
        );
    }
    let ep_cap = Capability {
        kind: CapabilityKind::Endpoint(id),
        rights: Rights::SEND | Rights::RECV | Rights::MANAGE,
    };
    let slot = ctx.tasks.current_caps_mut().allocate(ep_cap)?;
    Ok(slot)
}
pub(super) fn sys_cap_close(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    // Local drop only: remove the capability slot from the caller.
    //
    // Global endpoint close is handled by `sys_ipc_endpoint_close` (requires `Rights::MANAGE`).
    let cap = ctx.tasks.current_caps_mut().take(slot)?;
    // Release the backing kernel object for caps that own one (no dangling table entries).
    match cap.kind {
        CapabilityKind::Timer(id) => {
            let _ = ctx.hart_timers.free(crate::timer::TimerId(id));
        }
        CapabilityKind::Waitset(id) => {
            let _ = ctx.waitsets.free(crate::waitset::WaitsetId(id));
        }
        CapabilityKind::Fence(id) => {
            // Fence caps are transferable (workpool workers hold the parent's
            // fences): only the creator's close frees the kernel object; a
            // non-owner close drops just the slot so the owner's fence — and
            // every other holder's cap — stays valid.
            let fence_id = crate::fence::FenceId(id);
            if ctx.fences.owned_by(fence_id, ctx.tasks.current_pid().as_raw()) {
                let _ = ctx.fences.free(fence_id);
            }
        }
        _ => {}
    }
    Ok(0)
}

pub(super) fn sys_cap_clone(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let cap = ctx.tasks.current_caps_mut().get(slot)?;
    // Security floor: EndpointFactory must not be duplicable.
    if cap.kind == CapabilityKind::EndpointFactory {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let new_slot = ctx.tasks.current_caps_mut().allocate(cap)?;
    Ok(new_slot)
}

pub(super) fn sys_ipc_endpoint_close(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let cap = ctx.tasks.current_caps_mut().take(slot)?;
    let CapabilityKind::Endpoint(id) = cap.kind else {
        return Err(Error::Capability(CapError::InvalidSlot));
    };
    if !cap.rights.contains(Rights::MANAGE) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    #[cfg(feature = "ipc_trace_ring")]
    {
        crate::ipc::trace::record_ep_close(ctx.tasks.current_pid().as_raw(), id);
    }
    let waiters = ctx.router.close_endpoint(id)?;
    for pid in waiters {
        observe_wake_outcome(ctx.tasks.wake(task::Pid::from_raw(pid), ctx.scheduler));
    }
    Ok(0)
}

pub(super) fn sys_cap_query(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = SlotIndex::decode(args.get(0));
    let out_ptr = args.get(1);
    // out layout (LE):
    // - u32 kind_tag (1=vmo, 2=device_mmio)
    // - u32 reserved
    // - u64 base
    // - u64 len
    const OUT_LEN: usize = 24;
    ensure_user_slice(out_ptr, OUT_LEN)?;

    // Capability gate: require MAP rights to introspect address-bearing caps.
    let cap = ctx.tasks.current_caps_mut().derive(slot.0, Rights::MAP)?;
    let (kind_tag, base, len) = match cap.kind {
        CapabilityKind::Vmo { base, len } => (1u32, base as u64, len as u64),
        CapabilityKind::DeviceMmio { base, len } => (2u32, base as u64, len as u64),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };

    let mut out = [0u8; OUT_LEN];
    out[0..4].copy_from_slice(&kind_tag.to_le_bytes());
    out[4..8].copy_from_slice(&0u32.to_le_bytes());
    out[8..16].copy_from_slice(&base.to_le_bytes());
    out[16..24].copy_from_slice(&len.to_le_bytes());
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
    // RFC-0005 Phase 2 (hardening): `Rights::MANAGE` is not transferable for endpoints.
    //
    // Exceptions: EndpointFactory (init-lite holds endpoint-create authority)
    // and Fence caps (Phase C: fences carry only MANAGE and their signal/wait
    // authority is harmless — a workpool parent hands its fence caps to its
    // own compute threads before resuming them).
    if rights.contains(Rights::MANAGE) {
        let parent_caps =
            ctx.tasks.caps_of(parent).ok_or(Error::Transfer(task::TransferError::InvalidParent))?;
        let base = parent_caps
            .get(typed.parent_slot.0)
            .map_err(|e| Error::Transfer(task::TransferError::Capability(e)))?;
        if !matches!(base.kind, CapabilityKind::EndpointFactory | CapabilityKind::Fence(_)) {
            return Err(Error::Transfer(task::TransferError::Capability(
                CapError::PermissionDenied,
            )));
        }
    }

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
    let slot = ctx.tasks.transfer_cap(parent, typed.child, typed.parent_slot.0, rights)?;
    Ok(slot)
}

pub(super) fn sys_cap_transfer_to(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = CapTransferToArgsTyped::decode(args)?;
    let rights = typed.check()?;
    let parent = ctx.tasks.current_pid();
    // RFC-0005 Phase 2 (hardening): `Rights::MANAGE` is not transferable for endpoints.
    //
    // Exceptions: EndpointFactory (init-lite holds endpoint-create authority)
    // and Fence caps (Phase C: fences carry only MANAGE and their signal/wait
    // authority is harmless — a workpool parent hands its fence caps to its
    // own compute threads before resuming them).
    if rights.contains(Rights::MANAGE) {
        let parent_caps =
            ctx.tasks.caps_of(parent).ok_or(Error::Transfer(task::TransferError::InvalidParent))?;
        let base = parent_caps
            .get(typed.parent_slot.0)
            .map_err(|e| Error::Transfer(task::TransferError::Capability(e)))?;
        if !matches!(base.kind, CapabilityKind::EndpointFactory | CapabilityKind::Fence(_)) {
            return Err(Error::Transfer(task::TransferError::Capability(
                CapError::PermissionDenied,
            )));
        }
    }

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
    ctx.tasks.transfer_cap_to_slot(
        parent,
        typed.child,
        typed.parent_slot.0,
        rights,
        typed.child_slot.0,
    )?;
    Ok(typed.child_slot.0)
}
