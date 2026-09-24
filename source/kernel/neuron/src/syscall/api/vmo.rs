// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Memory syscalls split out of the former single-file api.rs:
//! sys_device_cap_create, sys_vmo_* (create/destroy/read/write/share) over the
//! page-backed objects of `mm::vmo` (TASK-0286 P3a — the arena and `VmoPool`
//! are gone), sys_as_create/sys_as_map, and user-slice validation helpers.
//! (The fixed-VA sys_map/sys_mmio_map were retired by RFC-0085 P6.)
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: neuron host tests + QEMU marker gates (just test-os / ci-os-smp)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::*;

// Typed decoders for the Decode→Check→Execute syscall discipline

#[derive(Copy, Clone)]
pub(super) struct AsMapArgsTyped {
    handle: AsHandle,
    vmo_slot: SlotIndex,
    va: VirtAddr,
    len: PageLen,
    prot: u32,
    flags: u32,
}

impl AsMapArgsTyped {
    #[inline]
    pub(super) fn decode(args: &Args) -> Result<Self, Error> {
        let handle =
            AsHandle::from_raw(args.get(0) as u32).ok_or(AddressSpaceError::InvalidHandle)?;
        let vmo_slot = SlotIndex::decode(args.get(1));
        let va = VirtAddr::page_aligned(args.get(2)).ok_or(AddressSpaceError::InvalidArgs)?;
        let len = PageLen::from_bytes_aligned(args.get(3) as u64)
            .ok_or(AddressSpaceError::InvalidArgs)?;
        let prot = args.get(4) as u32;
        let flags = args.get(5) as u32;
        Ok(Self { handle, vmo_slot, va, len, prot, flags })
    }

    #[inline]
    pub(super) fn check(&self) -> Result<(), Error> {
        if self.len.raw() == 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        // W^X
        if (self.prot & PROT_WRITE != 0) && (self.prot & PROT_EXEC != 0) {
            return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
        }
        // Range check: ensure va + len fits
        self.va.checked_add(self.len.raw()).ok_or(AddressSpaceError::InvalidArgs)?;
        Ok(())
    }
}

#[derive(Copy, Clone)]
pub(super) struct DeviceCapCreateArgsTyped {
    base: usize,
    len: usize,
    slot_raw: usize,
    /// The device's PLIC line from the tree's `interrupts` (0 = none).
    irq: usize,
    /// Arg 4: the flag word (`crate::dma_runs::device_flags`).
    flags: usize,
}

impl DeviceCapCreateArgsTyped {
    #[inline]
    pub(super) fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            base: args.get(0),
            len: args.get(1),
            slot_raw: args.get(2),
            irq: args.get(3),
            flags: args.get(4),
        })
    }
    #[inline]
    pub(super) fn check(&self) -> Result<(), Error> {
        if self.len == 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        // Deny-by-default on the flag word: a bit nobody defined is refused.
        crate::dma_runs::device_flags(self.flags).ok_or(AddressSpaceError::InvalidArgs)?;
        // A line the PLIC cannot have is a corrupt tree, not a device.
        if self.irq > crate::hal::plic::MAX_IRQ as usize {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        if (self.base & (PAGE_SIZE - 1)) != 0 || (self.len & (PAGE_SIZE - 1)) != 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        let end = self.base.checked_add(self.len).ok_or(AddressSpaceError::InvalidArgs)?;
        if end <= self.base {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
pub(super) struct VmoCreateArgsTyped {
    slot_raw: usize,
    len: usize,
    /// Arg 2 bit 0 (RFC-0098 C4): ONE physically contiguous block — the DMA
    /// masters' kind; `cap_query` names its base.
    contiguous: bool,
}

/// `vmo_create` arg 2: the object must be one physically contiguous block.
pub const VMO_CREATE_CONTIGUOUS: usize = 1;

impl VmoCreateArgsTyped {
    #[inline]
    pub(super) fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot_raw: args.get(0),
            len: args.get(1),
            contiguous: args.get(2) & VMO_CREATE_CONTIGUOUS != 0,
        })
    }
    #[inline]
    pub(super) fn check(&self) -> Result<(), Error> {
        if self.len == 0 || align_len(self.len).is_none() {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        Ok(())
    }
    fn kind(&self) -> crate::mm::vmo::VmoKind {
        if self.contiguous {
            crate::mm::vmo::VmoKind::Contiguous
        } else {
            crate::mm::vmo::VmoKind::Anon
        }
    }
}

#[derive(Copy, Clone)]
pub(super) struct VmoWriteArgsTyped {
    slot: SlotIndex,
    offset: usize,
    user_ptr: usize,
    len: usize,
}

impl VmoWriteArgsTyped {
    #[inline]
    pub(super) fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot: SlotIndex::decode(args.get(0)),
            offset: args.get(1),
            user_ptr: args.get(2),
            len: args.get(3),
        })
    }
    #[inline]
    pub(super) fn check(&self) -> Result<(), Error> {
        Ok(())
    }
}

pub(super) fn sys_device_cap_create(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = DeviceCapCreateArgsTyped::decode(args)?;
    typed.check()?;

    // Privileged gate: require EndpointFactory with MANAGE (init-lite only).
    let factory_cap = ctx
        .tasks
        .current_caps_mut()
        .get(1)
        .map_err(|_| Error::Capability(CapError::PermissionDenied))?;
    if factory_cap.kind != CapabilityKind::EndpointFactory
        || !factory_cap.rights.contains(Rights::MANAGE)
    {
        return Err(Error::Capability(CapError::PermissionDenied));
    }

    let cap = Capability {
        kind: CapabilityKind::DeviceMmio {
            base: typed.base,
            len: typed.len,
            irq: typed.irq as u32,
            dma_noncoherent: crate::dma_runs::device_flags(typed.flags).unwrap_or(false),
        },
        rights: Rights::MAP,
    };
    let slot = if typed.slot_raw == usize::MAX {
        ctx.tasks.current_caps_mut().allocate(cap)?
    } else {
        ctx.tasks.current_caps_mut().set(typed.slot_raw, cap)?;
        typed.slot_raw
    };
    Ok(slot)
}

/// Phase A (under the BKL): decode + take the frames for `SYSCALL_VMO_CREATE`.
/// Returns `(id, aligned_len, slot_raw)`; the frames are NOT zeroed yet.
pub(crate) fn vmo_create_reserve(args: &Args) -> Result<(u32, usize, usize), Error> {
    let typed = VmoCreateArgsTyped::decode(args)?;
    typed.check()?;
    let aligned = align_len(typed.len).ok_or(Error::Capability(CapError::PermissionDenied))?;
    let id = crate::mm::vmo::create(aligned, typed.kind())
        .map_err(|_| Error::Capability(CapError::PermissionDenied))?;
    Ok((id, aligned, typed.slot_raw))
}

/// Phase C (BKL re-acquired): install the capability. On failure the (now
/// zeroed) object goes back to the pool.
pub(crate) fn vmo_create_finish(
    ctx: &mut Context<'_>,
    id: u32,
    aligned: usize,
    slot_raw: usize,
) -> SysResult<usize> {
    let cap = Capability { kind: CapabilityKind::Vmo { id, len: aligned }, rights: Rights::MAP };
    let result = if slot_raw == usize::MAX {
        ctx.tasks.current_caps_mut().allocate(cap)
    } else {
        ctx.tasks.current_caps_mut().set(slot_raw, cap).map(|_| slot_raw)
    };
    if result.is_err() {
        let _ = crate::mm::vmo::destroy(id);
    }
    result.map_err(Into::into)
}

/// Unphased `SYSCALL_VMO_CREATE` (selftest-owned dispatch tables): the three
/// phases back to back, the zeroing under the BKL.
pub(super) fn sys_vmo_create(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let (id, aligned, slot_raw) = vmo_create_reserve(args)?;
    crate::mm::vmo::zero(id);
    vmo_create_finish(ctx, id, aligned, slot_raw)
}

/// `SYSCALL_VMO_DESTROY` (44): return a task-owned VMO's frames to the pool
/// (task #124: dead one-shot VMOs like the 4MB boot-splash backing must not
/// leak). Contract: for self-created, never-shared VMOs. The kernel refuses
/// while any OTHER capability anywhere in the system names the object
/// (clone/transfer alias) — the sole-owner safety net — and while any address
/// space still maps it (RFC-0085, EBUSY): freeing under a live mapping would
/// hand recycled frames to the old mapper. A new object is zeroed on create,
/// so no stale data ever leaks to the next owner.
pub(super) fn sys_vmo_destroy(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let cap = ctx.tasks.current_caps_mut().get(slot)?;
    let CapabilityKind::Vmo { id, .. } = cap.kind else {
        return Err(Error::Capability(CapError::PermissionDenied));
    };
    let mut refs = 0usize;
    for raw in 0..ctx.tasks.len() as u32 {
        if let Some(caps) = ctx.tasks.caps_of(task::Pid::from_raw(raw)) {
            refs += caps.vmo_ref_count(id);
        }
    }
    if refs != 1 {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    if crate::mm::vm_ops::any_space_maps_vmo(ctx.address_spaces, id) {
        return Err(Error::ResourceBusy);
    }
    let _ = ctx.tasks.current_caps_mut().take(slot)?;
    crate::mm::vmo::destroy(id).map_err(|_| Error::Capability(CapError::PermissionDenied))?;
    Ok(0)
}

/// `SYSCALL_VMO_SHARE_RO` (51, RFC-0080): derives a READ-ONLY alias
/// (`VmoRo`) of the `Vmo` in `slot` — same physical base/len — into a fresh
/// caller slot, returned as the result. Holders of the alias can only map it
/// read-only (`sys_map` strips WRITE|EXECUTE) and cannot `vmo_write` it. The
/// owner keeps the writable `Vmo` (to fill it); it grants clones of the alias.
pub(super) fn sys_vmo_share_ro(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let cap = ctx.tasks.current_caps_mut().get(slot)?;
    // Only a real (writable) VMO can be downgraded; require MAP authority.
    let CapabilityKind::Vmo { id, len } = cap.kind else {
        return Err(Error::Capability(CapError::PermissionDenied));
    };
    if !cap.rights.contains(Rights::MAP) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let ro = Capability { kind: CapabilityKind::VmoRo { id, len }, rights: Rights::MAP };
    let new_slot = ctx.tasks.current_caps_mut().allocate(ro)?;
    Ok(new_slot)
}

/// `SYSCALL_VMO_READ` (47): bounded copy OUT of a VMO into a caller buffer —
/// the exact mirror of `sys_vmo_write`. Requires the same `Rights::MAP`
/// derivation on the VMO capability; offsets/lengths are checked against the
/// VMO span and the destination is validated as a user slice. The ADR-0042
/// compositor damage-blit is the first consumer (windowd reads app surface
/// pixels; userspace has no VMO mapping path).
pub(super) fn sys_vmo_read(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = VmoWriteArgsTyped::decode(args)?;
    typed.check()?;
    let cap = ctx.tasks.current_caps_mut().derive(typed.slot.0, Rights::MAP)?;
    let (id, vmo_len) = match cap.kind {
        CapabilityKind::Vmo { id, len } => (id, len),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    let span_end =
        typed.offset.checked_add(typed.len).ok_or(Error::Capability(CapError::PermissionDenied))?;
    if span_end > vmo_len {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    ensure_user_slice(typed.user_ptr, typed.len)?;
    // Object bytes are runs; the user buffer is one range: copy run by run.
    let mut dst = typed.user_ptr;
    for (pa, len) in vmo_runs_in(id, typed.offset, typed.len)? {
        unsafe {
            ptr::copy_nonoverlapping(
                crate::phys::phys_to_virt(pa) as *const u8,
                dst as *mut u8,
                len,
            );
        }
        dst += len;
    }
    Ok(typed.len)
}

pub(super) fn sys_vmo_write(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = VmoWriteArgsTyped::decode(args)?;
    typed.check()?;
    let cap = ctx.tasks.current_caps_mut().derive(typed.slot.0, Rights::MAP)?;
    let (id, vmo_len) = match cap.kind {
        CapabilityKind::Vmo { id, len } => (id, len),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    #[cfg(feature = "debug_uart")]
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = write!(
            u,
            "VMO-WRITE slot=0x{:x} id={} off=0x{:x} len=0x{:x} user=0x{:x}\n",
            typed.slot.0, id, typed.offset, typed.len, typed.user_ptr
        );
    }
    let span_end =
        typed.offset.checked_add(typed.len).ok_or(Error::Capability(CapError::PermissionDenied))?;
    if span_end > vmo_len {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    ensure_user_slice(typed.user_ptr, typed.len)?;
    #[cfg(feature = "debug_uart")]
    let preview_len = core::cmp::min(typed.len, 16);
    #[cfg(feature = "debug_uart")]
    let mut preview_bytes = [0u8; 16];
    #[cfg(feature = "debug_uart")]
    if preview_len > 0 {
        unsafe {
            ptr::copy_nonoverlapping(
                typed.user_ptr as *const u8,
                preview_bytes.as_mut_ptr(),
                preview_len,
            );
        }
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ =
            write!(u, "VMO-WRITE DATA slot=0x{:x} off=0x{:x} head=0x", typed.slot.0, typed.offset);
        for byte in preview_bytes.iter().take(preview_len) {
            let _ = write!(u, "{:02x}", byte);
        }
        let _ = u.write_str("\n");
    }
    let mut src = typed.user_ptr;
    for (pa, len) in vmo_runs_in(id, typed.offset, typed.len)? {
        unsafe {
            ptr::copy_nonoverlapping(
                src as *const u8,
                crate::phys::phys_to_virt(pa) as *mut u8,
                len,
            );
        }
        src += len;
    }
    if typed.len != 0 {
        riscv::asm::fence_i();
    }
    Ok(typed.len)
}

pub(super) const PROT_READ: u32 = 1 << 0;
pub(super) const PROT_WRITE: u32 = 1 << 1;
pub(super) const PROT_EXEC: u32 = 1 << 2;

pub(super) const MAP_FLAG_USER: u32 = 1 << 0;
pub(super) const USER_VADDR_LIMIT: usize = 0x8000_0000;

pub(super) fn align_len(len: usize) -> Option<usize> {
    if len == 0 {
        Some(0)
    } else {
        len.checked_add(PAGE_SIZE - 1).map(|value| value & !(PAGE_SIZE - 1))
    }
}

pub(super) fn ensure_user_slice(ptr: usize, len: usize) -> Result<(), Error> {
    if len == 0 {
        return Ok(());
    }

    // Host tests run the kernel logic in-process; pointers won't fall under the Sv39 user VA range.
    // For tests, accept any non-overflowing slice address and rely on Rust/host memory safety.
    #[cfg(test)]
    {
        let _last = ptr.checked_add(len - 1).ok_or(AddressSpaceError::InvalidArgs)?;
        return Ok(());
    }

    // Non-test (real kernel): enforce Sv39 user VA range and reject null pointers.
    #[cfg(not(test))]
    {
        if ptr == 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        if ptr >= USER_VADDR_LIMIT {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        let last = ptr.checked_add(len - 1).ok_or(AddressSpaceError::InvalidArgs)?;
        if last >= USER_VADDR_LIMIT {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(())
    }
}

pub(super) fn sys_as_create(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    let handle = ctx.address_spaces.create()?;
    Ok(handle.to_raw() as usize)
}

pub(super) fn sys_as_map(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = AsMapArgsTyped::decode(args)?;
    typed.check()?; // Check phase

    let cap = ctx.tasks.current_caps_mut().derive(typed.vmo_slot.0, Rights::MAP)?;
    let (id, vmo_len) = match cap.kind {
        CapabilityKind::Vmo { id, len } => (id, len as u64),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };

    let map_bytes = cmp::min(typed.len.raw() as u64, vmo_len);
    let aligned_bytes = map_bytes - (map_bytes % PAGE_SIZE as u64);
    if aligned_bytes == 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let pages = (aligned_bytes / PAGE_SIZE as u64) as usize;
    let span_bytes = pages.checked_mul(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
    typed.va.checked_add(span_bytes).ok_or(AddressSpaceError::InvalidArgs)?;

    let mut flags = PageFlags::VALID;
    if typed.prot & PROT_READ != 0 {
        flags |= PageFlags::READ;
    }
    if typed.prot & PROT_WRITE != 0 {
        flags |= PageFlags::WRITE;
    }
    if typed.prot & PROT_EXEC != 0 {
        flags |= PageFlags::EXECUTE;
    }
    if typed.flags & MAP_FLAG_USER != 0 {
        flags |= PageFlags::USER;
    }

    // RFC-0004: enforce W^X at the syscall boundary for user mappings.
    if flags.contains(PageFlags::WRITE) && flags.contains(PageFlags::EXECUTE) {
        return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
    }

    #[cfg(feature = "debug_uart")]
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = writeln!(
            u,
            "AS-MAP handle=0x{:x} slot=0x{:x} va=0x{:x} len=0x{:x} pages=0x{:x} id={} prot=0x{:x} flags=0x{:x}",
            typed.handle.to_raw(),
            typed.vmo_slot.0,
            typed.va.raw(),
            typed.len.raw(),
            pages,
            id,
            typed.prot,
            flags.bits()
        );
    }

    for page in 0..pages {
        let page_va =
            typed.va.raw().checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        let page_pa = crate::mm::vmo::translate(id, page * PAGE_SIZE)
            .ok_or(Error::Capability(CapError::PermissionDenied))?;
        ctx.address_spaces.map_page_tracked(typed.handle, page_va, page_pa, flags)?;
    }

    Ok(0)
}
