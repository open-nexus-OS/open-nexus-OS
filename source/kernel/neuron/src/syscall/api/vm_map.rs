// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The MAP surface of the syscall API — RFC-0085's
//! kernel-chosen-VA syscalls (`sys_vm_map`/`sys_vm_unmap` incl. the phased
//! twins/`sys_mmio_map_auto`). The fixed-VA ancestors (`sys_map` 4,
//! `sys_mmio_map` 27) are DELETED — numbers retired, never reused.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker gates; host oracle lands with RFC-0085's
//! `va_space` pure module
//! ADR: docs/adr/0054-map-errors-keep-their-identity-across-the-abi.md

use super::*;

/// `SYSCALL_VM_MAP` (53, RFC-0085): whole-range VMO map at a kernel-chosen
/// va. Args: (vmo_slot, offset, len, flags) → va. Own address space only.
pub(super) fn sys_vm_map(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = SlotIndex::decode(args.get(0));
    let offset = args.get(1);
    let len = args.get(2);
    let flags = PageFlags::from_bits(args.get(3)).ok_or(AddressSpaceError::InvalidArgs)?;
    // W^X at the boundary; EXECUTE has no vm_map consumer at all — refuse it
    // outright rather than waiting for the combination check.
    if flags.contains(PageFlags::EXECUTE) {
        return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
    }
    if len == 0 || len % PAGE_SIZE != 0 || offset % PAGE_SIZE != 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let cap = ctx.tasks.current_caps_mut().derive(slot.0, Rights::MAP)?;
    let (base, cap_len, read_only) = match cap.kind {
        CapabilityKind::Vmo { base, len } => (base, len, false),
        CapabilityKind::VmoRo { base, len } => (base, len, true),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    let span_end = offset.checked_add(len).ok_or(AddressSpaceError::InvalidArgs)?;
    if span_end > cap_len {
        log_error!(
            target: "sysmap",
            "VM-MAP-FAIL reason=offset-guard off=0x{:x} len=0x{:x} cap_len=0x{:x}",
            offset,
            len,
            cap_len
        );
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    // Security floor: kernel-chosen-va mappings exist FOR userspace — force
    // VALID|USER; an RO alias can never regain WRITE (host-proven policy).
    let mut flags = flags | PageFlags::VALID | PageFlags::USER;
    if read_only {
        flags = PageFlags::from_bits_truncate(crate::vmo_ro::force_readonly(flags.bits()));
    }
    let handle =
        ctx.tasks.current_task().address_space().ok_or(AddressSpaceError::InvalidHandle)?;
    let space = ctx.address_spaces.get_mut(handle)?;
    let va = crate::mm::vm_ops::map_range(
        space,
        base + offset,
        len,
        flags,
        crate::va_space::RegionKind::Vmo,
    )
    .map_err(Error::from)?;
    Ok(va)
}

/// `SYSCALL_VM_UNMAP` (54, RFC-0085): unmap ONE exact `vm_map`/
/// `mmio_map_auto` region. Args: (va, len). Single-phase fallback path
/// (selftest-owned dispatch tables); the trap layer routes the live
/// syscall through the PHASED twins below so the TLB shootdown wait never
/// holds the BKL.
pub(super) fn sys_vm_unmap(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let handle =
        ctx.tasks.current_task().address_space().ok_or(AddressSpaceError::InvalidHandle)?;
    let space = ctx.address_spaces.get_mut(handle)?;
    crate::mm::vm_ops::unmap_range(space, args.get(0), args.get(1)).map_err(Error::from)?;
    Ok(0)
}

/// Phase A of the phased vm_unmap (BKL held): validate + clear PTEs; the
/// region stays recorded so its va cannot be reused before the shootdown.
pub(crate) fn vm_unmap_clear(ctx: &mut Context<'_>, args: &Args) -> SysResult<()> {
    let handle =
        ctx.tasks.current_task().address_space().ok_or(AddressSpaceError::InvalidHandle)?;
    let space = ctx.address_spaces.get_mut(handle)?;
    crate::mm::vm_ops::clear_range(space, args.get(0), args.get(1)).map_err(Error::from)?;
    Ok(())
}

/// Phase C of the phased vm_unmap (BKL re-acquired, shootdown done):
/// forget the region.
pub(crate) fn vm_unmap_finish(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let handle =
        ctx.tasks.current_task().address_space().ok_or(AddressSpaceError::InvalidHandle)?;
    let space = ctx.address_spaces.get_mut(handle)?;
    crate::mm::vm_ops::forget_range(space, args.get(0), args.get(1));
    Ok(0)
}

/// `SYSCALL_MMIO_MAP_AUTO` (55, RFC-0085): device-MMIO window at a
/// kernel-chosen va. Args: (mmio_slot, offset, len) → va. Security floor
/// unchanged from the retired `sys_mmio_map`: USER|RW, never EXEC.
pub(super) fn sys_mmio_map_auto(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = SlotIndex::decode(args.get(0));
    let offset = args.get(1);
    let len = args.get(2);
    if len == 0 || len % PAGE_SIZE != 0 || offset % PAGE_SIZE != 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let cap = ctx.tasks.current_caps_mut().derive(slot.0, Rights::MAP)?;
    let (base, cap_len) = match cap.kind {
        CapabilityKind::DeviceMmio { base, len } => (base, len),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    let span_end = offset.checked_add(len).ok_or(AddressSpaceError::InvalidArgs)?;
    if span_end > cap_len {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ | PageFlags::WRITE;
    let handle =
        ctx.tasks.current_task().address_space().ok_or(AddressSpaceError::InvalidHandle)?;
    let space = ctx.address_spaces.get_mut(handle)?;
    let va = crate::mm::vm_ops::map_range(
        space,
        base + offset,
        len,
        flags,
        crate::va_space::RegionKind::Mmio,
    )
    .map_err(Error::from)?;
    Ok(va)
}
