// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Memory syscalls split out of the former single-file api.rs:
//! sys_device_cap_create, sys_vmo_* (create/destroy/read/write),
//! sys_as_create/sys_as_map, the kernel-managed user VMO arena
//! (VMO_POOL/VmoPool, task #124 free list) and user-slice validation helpers.
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
}

impl DeviceCapCreateArgsTyped {
    #[inline]
    pub(super) fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self { base: args.get(0), len: args.get(1), slot_raw: args.get(2) })
    }
    #[inline]
    pub(super) fn check(&self) -> Result<(), Error> {
        if self.len == 0 {
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
}

impl VmoCreateArgsTyped {
    #[inline]
    pub(super) fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self { slot_raw: args.get(0), len: args.get(1) })
    }
    #[inline]
    pub(super) fn check(&self) -> Result<(), Error> {
        if self.len == 0 {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        Ok(())
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
        kind: CapabilityKind::DeviceMmio { base: typed.base, len: typed.len },
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

/// P2 phase A (under the BKL): decode + reserve for `SYSCALL_VMO_CREATE`.
/// Returns (base, aligned_len, needs_zero, slot_raw).
pub(crate) fn vmo_create_reserve(args: &Args) -> Result<(usize, usize, bool, usize), Error> {
    let typed = VmoCreateArgsTyped::decode(args)?;
    typed.check()?;
    let (base, aligned, needs_zero) = VMO_POOL.lock().allocate_nozero(typed.len)?;
    Ok((base, aligned, needs_zero, typed.slot_raw))
}

/// P2 phase C (BKL re-acquired): install the capability. On failure the
/// (now zeroed) range goes back CLEAN via the free list.
pub(crate) fn vmo_create_finish(
    ctx: &mut Context<'_>,
    base: usize,
    aligned: usize,
    slot_raw: usize,
) -> SysResult<usize> {
    let cap = Capability { kind: CapabilityKind::Vmo { base, len: aligned }, rights: Rights::MAP };
    let result = if slot_raw == usize::MAX {
        ctx.tasks.current_caps_mut().allocate(cap)
    } else {
        ctx.tasks.current_caps_mut().set(slot_raw, cap).map(|_| slot_raw)
    };
    if result.is_err() {
        let _ = VMO_POOL.lock().free(base, aligned);
    }
    result.map_err(Into::into)
}

/// P2: one bounded idle-zero step (64 KiB) — cpu_main idle hook.
pub fn vmo_idle_zero_step() -> usize {
    VMO_POOL.lock().idle_zero_step(64 * 1024)
}

pub(super) fn sys_vmo_create(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = VmoCreateArgsTyped::decode(args)?;
    typed.check()?;
    let (base, aligned_len) = VMO_POOL.lock().allocate(typed.len)?;
    #[cfg(feature = "debug_uart")]
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = writeln!(
            u,
            "VMO-CREATE len=0x{:x} base=0x{:x} slot=0x{:x}",
            aligned_len, base, typed.slot_raw
        );
    }
    let cap =
        Capability { kind: CapabilityKind::Vmo { base, len: aligned_len }, rights: Rights::MAP };
    let target = if typed.slot_raw == usize::MAX {
        ctx.tasks.current_caps_mut().allocate(cap)?
    } else {
        ctx.tasks.current_caps_mut().set(typed.slot_raw, cap)?;
        typed.slot_raw
    };
    Ok(target)
}

/// `SYSCALL_VMO_DESTROY` (44): release a task-owned VMO back to the kernel arena
/// (task #124 — the arena was bump-only; dead one-shot VMOs like the 4MB
/// boot-splash backing leaked forever). Contract: for self-created, never-shared
/// VMOs. The kernel refuses while any OTHER capability anywhere in the system
/// references the range (clone/transfer alias) — the sole-owner safety net.
/// Mappings are the caller's contract: it must not touch the range afterwards
/// (a stale writable mapping in the destroying task could scribble over a reused
/// range — the same trust already granted by `vm_map` on its own VMOs; the
/// arena zeroes on reuse, so no stale data ever leaks to the next owner).
pub(super) fn sys_vmo_destroy(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let cap = ctx.tasks.current_caps_mut().get(slot)?;
    let CapabilityKind::Vmo { base, len } = cap.kind else {
        return Err(Error::Capability(CapError::PermissionDenied));
    };
    let mut refs = 0usize;
    for raw in 0..ctx.tasks.len() as u32 {
        if let Some(caps) = ctx.tasks.caps_of(task::Pid::from_raw(raw)) {
            refs += caps.vmo_overlap_count(base, len);
        }
    }
    if refs != 1 {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    // RFC-0085: refuse while ANY address space still maps the range through a
    // vm_map/mmio_map_auto region (EBUSY) — freeing under a live mapping
    // would hand recycled arena pages to the old mapper. Legacy fixed-VA
    // maps keep the pre-RFC caller contract (recorded as Fixed, not counted
    // here).
    if crate::mm::vm_ops::any_space_maps(ctx.address_spaces, base, len) {
        return Err(Error::ResourceBusy);
    }
    let _ = ctx.tasks.current_caps_mut().take(slot)?;
    VMO_POOL.lock().free(base, len)?;
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
    let CapabilityKind::Vmo { base, len } = cap.kind else {
        return Err(Error::Capability(CapError::PermissionDenied));
    };
    if !cap.rights.contains(Rights::MAP) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let ro = Capability { kind: CapabilityKind::VmoRo { base, len }, rights: Rights::MAP };
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
    let (base, vmo_len) = match cap.kind {
        CapabilityKind::Vmo { base, len } => (base, len),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    let span_end =
        typed.offset.checked_add(typed.len).ok_or(Error::Capability(CapError::PermissionDenied))?;
    if span_end > vmo_len {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    ensure_user_slice(typed.user_ptr, typed.len)?;
    if typed.len != 0 {
        unsafe {
            ptr::copy_nonoverlapping(
                (base + typed.offset) as *const u8,
                typed.user_ptr as *mut u8,
                typed.len,
            );
        }
    }
    Ok(typed.len)
}

pub(super) fn sys_vmo_write(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = VmoWriteArgsTyped::decode(args)?;
    typed.check()?;
    let cap = ctx.tasks.current_caps_mut().derive(typed.slot.0, Rights::MAP)?;
    let (base, vmo_len) = match cap.kind {
        CapabilityKind::Vmo { base, len } => (base, len),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    #[cfg(feature = "debug_uart")]
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = write!(
            u,
            "VMO-WRITE slot=0x{:x} base=0x{:x} off=0x{:x} len=0x{:x} user=0x{:x}\n",
            typed.slot.0, base, typed.offset, typed.len, typed.user_ptr
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
    if typed.len != 0 {
        unsafe {
            ptr::copy_nonoverlapping(
                typed.user_ptr as *const u8,
                (base + typed.offset) as *mut u8,
                typed.len,
            );
            riscv::asm::fence_i();
        }
    }
    Ok(typed.len)
}

pub(super) const PROT_READ: u32 = 1 << 0;
pub(super) const PROT_WRITE: u32 = 1 << 1;
pub(super) const PROT_EXEC: u32 = 1 << 2;

pub(super) const MAP_FLAG_USER: u32 = 1 << 0;
pub(super) const USER_VADDR_LIMIT: usize = 0x8000_0000;

// The kernel-managed user VMO arena (`VMO_POOL`/`VmoPool`) lives in
// `vmo_pool.rs` (module-size ratchet); `api/mod.rs` globs it so `sys_vmo_*`
// below and `exec`/`tests` reach `VMO_POOL`/`VmoPool` unqualified.
pub(super) fn align_len(len: usize) -> Option<usize> {
    if len == 0 {
        Some(0)
    } else {
        len.checked_add(PAGE_SIZE - 1).map(|value| value & !(PAGE_SIZE - 1))
    }
}

pub(super) fn align_up_addr(addr: usize) -> usize {
    let mask = PAGE_SIZE - 1;
    (addr + mask) & !mask
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
    let (base, vmo_len) = match cap.kind {
        CapabilityKind::Vmo { base, len } => (base, len as u64),
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
            "AS-MAP handle=0x{:x} slot=0x{:x} va=0x{:x} len=0x{:x} pages=0x{:x} base=0x{:x} prot=0x{:x} flags=0x{:x}",
            typed.handle.to_raw(),
            typed.vmo_slot.0,
            typed.va.raw(),
            typed.len.raw(),
            pages,
            base,
            typed.prot,
            flags.bits()
        );
    }

    #[cfg(feature = "debug_uart")]
    let mut logged_preview = false;

    for page in 0..pages {
        let page_va =
            typed.va.raw().checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        let page_pa = base.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        ctx.address_spaces.map_page_tracked(typed.handle, page_va, page_pa, flags)?;
        #[cfg(feature = "debug_uart")]
        if !logged_preview {
            logged_preview = true;
            log_vmo_preview(typed.vmo_slot.0, page_pa, aligned_bytes, typed.prot);
        }
    }

    Ok(0)
}

#[cfg(feature = "debug_uart")]
pub(super) fn log_vmo_preview(slot: usize, base: usize, len: u64, prot: u32) {
    use core::fmt::Write as _;

    let mut u = crate::uart::raw_writer();
    let preview_len = core::cmp::min(len, 16) as usize;

    let pool = VMO_POOL.lock();
    let in_pool = preview_len > 0 && pool.contains(base, preview_len);
    drop(pool);

    if !in_pool {
        let _ = write!(
            u,
            "VMO-PREVIEW skipped slot=0x{:x} base=0x{:x} len=0x{:x} prot=0x{:x}\n",
            slot, base, len, prot
        );
        return;
    }

    let mut buf = [0u8; 16];
    if preview_len > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(base as *const u8, buf.as_mut_ptr(), preview_len);
        }
    }
    let _ = write!(
        u,
        "VMO-PREVIEW slot=0x{:x} base=0x{:x} len=0x{:x} prot=0x{:x} bytes=",
        slot, base, len, prot
    );
    for byte in &buf[..preview_len] {
        let _ = write!(u, "{:02x}", byte);
    }
    let _ = u.write_str("\n");
}
