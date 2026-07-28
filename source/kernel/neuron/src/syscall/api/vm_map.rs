// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The MAP surface of the syscall API — `sys_map` (fixed-VA VMO
//! page map, the ELF-loader-era primitive) and `sys_mmio_map` (device MMIO
//! window), with their seL4-style typed decoders and the TASK-0309 stage
//! tracing (`MAP-FAIL stage=…`). Split out of `vmo.rs` (structure-gate);
//! RFC-0085's kernel-chosen-VA syscalls (`sys_vm_map`/`sys_vm_unmap`/
//! `sys_mmio_map_auto`) land here beside them.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker gates; host oracle lands with RFC-0085's
//! `va_space` pure module
//! ADR: docs/adr/0054-map-errors-keep-their-identity-across-the-abi.md

use super::*;

#[derive(Copy, Clone)]
pub(super) struct MapArgsTyped {
    slot: SlotIndex,
    va: VirtAddr,
    offset: usize,
    flags: PageFlags,
}

impl MapArgsTyped {
    #[inline]
    pub(super) fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot: SlotIndex::decode(args.get(0)),
            va: VirtAddr::page_aligned(args.get(1)).ok_or(AddressSpaceError::InvalidArgs)?,
            offset: args.get(2),
            flags: PageFlags::from_bits(args.get(3)).ok_or(AddressSpaceError::InvalidArgs)?,
        })
    }
    #[inline]
    pub(super) fn check(&self) -> Result<(), Error> {
        if self.flags.contains(PageFlags::WRITE) && self.flags.contains(PageFlags::EXECUTE) {
            return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
pub(super) struct MmioMapArgsTyped {
    slot: SlotIndex,
    va: VirtAddr,
    offset: usize,
}

impl MmioMapArgsTyped {
    #[inline]
    pub(super) fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot: SlotIndex::decode(args.get(0)),
            va: VirtAddr::page_aligned(args.get(1)).ok_or(AddressSpaceError::InvalidArgs)?,
            offset: args.get(2),
        })
    }
    #[inline]
    pub(super) fn check(&self) -> Result<(), Error> {
        // Additional bounds checks are performed against the capability window in the handler.
        Ok(())
    }
}

/// Names the failing STAGE of a refused `SYSCALL_MAP` (TASK-0309). The map
/// loop for a large VMO issues thousands of identical calls; when one of them
/// fails mid-run, the errno alone cannot say whether decode, capability
/// derivation, the offset guard or the page table refused — and each of those
/// implicates a different owner. Error path only.
fn trace_map_fail(stage: &str, va: usize, offset: usize, err: &Error) {
    log_error!(
        target: "sysmap",
        "MAP-FAIL stage={} va=0x{:x} off=0x{:x} err={:?}",
        stage,
        va,
        offset,
        err
    );
}

pub(super) fn sys_map(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = MapArgsTyped::decode(args).inspect_err(|e| {
        trace_map_fail("decode", args.get(1), args.get(2), e);
    })?;
    typed.check().inspect_err(|e| {
        trace_map_fail("check", typed.va.raw(), typed.offset, e);
    })?;
    let cap = ctx.tasks.current_caps_mut().derive(typed.slot.0, Rights::MAP).inspect_err(|e| {
        trace_map_fail("derive", typed.va.raw(), typed.offset, &Error::Capability(*e));
    })?;
    match cap.kind {
        // RFC-0080: a read-only VMO alias force-maps read-only — WRITE|EXECUTE
        // are stripped so the holder can never mutate the shared pages. Same
        // physical resolution as `Vmo` otherwise.
        CapabilityKind::Vmo { base, len } | CapabilityKind::VmoRo { base, len } => {
            let read_only = matches!(cap.kind, CapabilityKind::VmoRo { .. });
            if typed.offset >= len {
                // The one refusal a large-VMO map loop is LIKELY to hit: the
                // caller's offset ran past the capability's length. Log the
                // cap length too — the caller knows its offset, only the
                // kernel knows what this capability's `len` actually is.
                log_error!(
                    target: "sysmap",
                    "MAP-FAIL stage=offset-guard va=0x{:x} off=0x{:x} cap_len=0x{:x}",
                    typed.va.raw(),
                    typed.offset,
                    len
                );
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            let flags = if read_only {
                // Pure, host-tested RO policy: WRITE|EXECUTE can never survive.
                PageFlags::from_bits_truncate(crate::vmo_ro::force_readonly(typed.flags.bits()))
            } else {
                typed.flags
            };
            let va = typed.va;
            let pa = base + (typed.offset & !0xfff);
            let handle = ctx
                .tasks
                .current_task()
                .address_space()
                .ok_or(AddressSpaceError::InvalidHandle)
                .inspect_err(|_| {
                    log_error!(
                        target: "sysmap",
                        "MAP-FAIL stage=as-handle va=0x{:x} off=0x{:x}",
                        typed.va.raw(),
                        typed.offset
                    );
                })?;
            #[cfg(feature = "debug_uart")]
            {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = writeln!(
                    u,
                    "AS-MAP handle=0x{:x} va=0x{:x} pa=0x{:x} flags=0x{:x}",
                    handle.to_raw(),
                    va.raw(),
                    pa,
                    flags.bits()
                );
            }
            ctx.address_spaces.map_page_tracked(handle, va.raw(), pa, flags).inspect_err(|e| {
                trace_map_fail("map-page", va.raw(), typed.offset, &Error::AddressSpace(*e));
            })?;
            Ok(0)
        }
        _ => {
            log_error!(
                target: "sysmap",
                "MAP-FAIL stage=cap-kind va=0x{:x} off=0x{:x}",
                typed.va.raw(),
                typed.offset
            );
            Err(Error::Capability(CapError::PermissionDenied))
        }
    }
}

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
/// `mmio_map_auto` region. Args: (va, len).
pub(super) fn sys_vm_unmap(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let handle =
        ctx.tasks.current_task().address_space().ok_or(AddressSpaceError::InvalidHandle)?;
    let space = ctx.address_spaces.get_mut(handle)?;
    crate::mm::vm_ops::unmap_range(space, args.get(0), args.get(1)).map_err(Error::from)?;
    Ok(0)
}

/// `SYSCALL_MMIO_MAP_AUTO` (55, RFC-0085): device-MMIO window at a
/// kernel-chosen va. Args: (mmio_slot, offset, len) → va. Security floor
/// unchanged from `sys_mmio_map`: USER|RW, never EXEC.
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

pub(super) fn sys_mmio_map(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = MmioMapArgsTyped::decode(args)?;
    typed.check()?;

    let cap = ctx.tasks.current_caps_mut().derive(typed.slot.0, Rights::MAP)?;

    let (base, len) = match cap.kind {
        CapabilityKind::DeviceMmio { base, len } => (base, len),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };

    if typed.offset >= len {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    // Enforce page-granularity offsets (per normative v1 contract).
    if (typed.offset & (PAGE_SIZE - 1)) != 0 {
        return Err(Error::Capability(CapError::PermissionDenied));
    }

    let handle =
        ctx.tasks.current_task().address_space().ok_or(AddressSpaceError::InvalidHandle)?;

    // Enforce the security floor at the boundary:
    // - USER + RW only
    // - never EXEC
    let flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ | PageFlags::WRITE;

    let pa =
        base.checked_add(typed.offset & !(PAGE_SIZE - 1)).ok_or(AddressSpaceError::InvalidArgs)?;

    ctx.address_spaces.map_page_tracked(handle, typed.va.raw(), pa, flags)?;
    Ok(0)
}
