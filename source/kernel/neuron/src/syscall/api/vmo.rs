// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Memory syscalls split out of the former single-file api.rs:
//! sys_map/sys_mmio_map/sys_device_cap_create, sys_vmo_* (create/destroy/
//! read/write), sys_as_create/sys_as_map, the kernel-managed user VMO arena
//! (VMO_POOL/VmoPool, task #124 free list) and user-slice validation helpers.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: neuron host tests + QEMU marker gates (just test-os / ci-os-smp)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::*;

// Typed decoders for seL4-style Decode→Check→Execute

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

pub(super) fn sys_map(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = MapArgsTyped::decode(args)?;
    typed.check()?;
    let cap = ctx.tasks.current_caps_mut().derive(typed.slot.0, Rights::MAP)?;
    match cap.kind {
        CapabilityKind::Vmo { base, len } => {
            if typed.offset >= len {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            let va = typed.va;
            let pa = base + (typed.offset & !0xfff);
            let handle =
                ctx.tasks.current_task().address_space().ok_or(AddressSpaceError::InvalidHandle)?;
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
                    typed.flags.bits()
                );
            }
            ctx.address_spaces.map_page(handle, va.raw(), pa, typed.flags)?;
            Ok(0)
        }
        _ => Err(Error::Capability(CapError::PermissionDenied)),
    }
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

    ctx.address_spaces.map_page(handle, typed.va.raw(), pa, flags)?;
    Ok(0)
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
/// range — the same trust already granted by `vmo_map_page` on its own VMOs; the
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
    let _ = ctx.tasks.current_caps_mut().take(slot)?;
    VMO_POOL.lock().free(base, len)?;
    Ok(0)
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

pub(super) static VMO_POOL: Mutex<VmoPool> = Mutex::new(VmoPool::new());

/// Bounded free-range table (task #124): freed one-shot VMOs (boot-splash
/// backing, staging buffers) are reused instead of growing the bump frontier.
pub(super) const VMO_FREE_SLOTS: usize = 16;

pub(super) struct VmoPool {
    base: usize,
    next: usize,
    pub(super) limit: usize,
    peak_next: usize,
    /// Freed ranges available for reuse: `(base, len)`, `len == 0` marks an
    /// empty entry. Bounded — when full, a freed range is leaked (graceful
    /// bump-only degradation), never corrupted.
    free_list: [(usize, usize); VMO_FREE_SLOTS],
    /// P2: freed-but-not-yet-zeroed ranges. `free()` parks ranges here; the
    /// idle harts zero them (`idle_zero_step`) and promote them to
    /// `free_list`, so `allocate`'s reuse path NEVER memsets (the recycled
    /// 4MB boot-splash VMO was the measured ~100ms BKL hold).
    dirty_list: [(usize, usize); VMO_FREE_SLOTS],
    /// Bytes dropped because the free list was full (observability).
    leaked: usize,
    /// Zero-frontier (P2): everything in [next, zeroed_until) is already
    /// zeroed by the idle harts (`idle_zero_step`), so the common
    /// `vmo_create` path skips its memset entirely — the measured 82-90ms
    /// BKL holds were exactly this zeroing running inside the syscall.
    zeroed_until: usize,
}

/// Snapshot of the kernel-managed user VMO arena.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct VmoPoolStats {
    pub(super) base: usize,
    pub(super) used: usize,
    pub(super) remaining: usize,
    pub(super) peak_used: usize,
}

impl VmoPool {
    pub(super) const fn new() -> Self {
        Self {
            base: 0,
            next: 0,
            limit: 0,
            peak_next: 0,
            free_list: [(0, 0); VMO_FREE_SLOTS],
            dirty_list: [(0, 0); VMO_FREE_SLOTS],
            leaked: 0,
            zeroed_until: 0,
        }
    }

    #[cfg(test)]
    pub(super) fn with_window(base: usize, len: usize) -> Self {
        Self {
            base,
            next: base,
            limit: base.saturating_add(len),
            peak_next: base,
            free_list: [(0, 0); VMO_FREE_SLOTS],
            dirty_list: [(0, 0); VMO_FREE_SLOTS],
            leaked: 0,
        }
    }

    pub(super) fn ensure_initialized(&mut self) {
        if self.base != 0 {
            return;
        }
        let start = align_up_addr(USER_VMO_ARENA_BASE);
        let limit = start.saturating_add(USER_VMO_ARENA_LEN);
        self.base = start;
        self.next = start;
        self.zeroed_until = start;
        self.limit = limit;
        self.peak_next = start;
    }

    /// P2 phased reserve: clean free-list first (no zero needed), else bump
    /// WITHOUT zeroing — the trap handler zeroes with the BKL dropped. The
    /// range is unreachable until `vmo_create_finish` installs the cap.
    /// Returns (base, len, needs_zero).
    pub(super) fn allocate_nozero(&mut self, len: usize) -> Result<(usize, usize, bool), Error> {
        self.ensure_initialized();
        if len == 0 {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        let aligned = align_len(len).ok_or(Error::Capability(CapError::PermissionDenied))?;
        let mut best: Option<usize> = None;
        for (index, &(_, entry_len)) in self.free_list.iter().enumerate() {
            if entry_len >= aligned && best.is_none_or(|prev| entry_len < self.free_list[prev].1) {
                best = Some(index);
            }
        }
        if let Some(index) = best {
            let (entry_base, entry_len) = self.free_list[index];
            self.free_list[index] = if entry_len > aligned {
                (entry_base + aligned, entry_len - aligned)
            } else {
                (0, 0)
            };
            return Ok((entry_base, aligned, false));
        }
        let next =
            self.next.checked_add(aligned).ok_or(Error::Capability(CapError::PermissionDenied))?;
        if next > self.limit {
            let pressure = self.stats();
            log_error!(
                "VMO-POOL exhausted: want=0x{:x} used=0x{:x} remaining=0x{:x} peak=0x{:x}",
                aligned,
                pressure.used,
                pressure.remaining,
                pressure.peak_used
            );
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        let base = self.next;
        self.next = next;
        if self.next > self.peak_next {
            self.peak_next = self.next;
        }
        // Pre-zeroed by the idle frontier? Then no zero pass is needed.
        let needs_zero = self.zeroed_until < base + aligned;
        if !needs_zero && self.zeroed_until < base + aligned {
            self.zeroed_until = base + aligned;
        }
        Ok((base, aligned, needs_zero))
    }

    /// P2 idle-hart pre-zeroing: advance the zero-frontier by up to
    /// `budget` bytes. Called from cpu_main's idle path (pool leaf lock only,
    /// never the BKL) — idle cpus pay the memset so vmo_create never does.
    /// Returns bytes zeroed (0 = frontier fully ahead, caller can WFI).
    pub(super) fn idle_zero_step(&mut self, budget: usize) -> usize {
        self.ensure_initialized();
        // Dirty recycled ranges first (they unblock allocate's clean reuse).
        for i in 0..self.dirty_list.len() {
            let (base, len) = self.dirty_list[i];
            if len == 0 {
                continue;
            }
            let n = len.min(budget);
            unsafe {
                ptr::write_bytes(base as *mut u8, 0, n);
            }
            if n == len {
                self.dirty_list[i] = (0, 0);
                // Promote to the clean free list (coalesce with neighbours).
                let mut base = base;
                let mut len = len;
                for entry in self.free_list.iter_mut() {
                    if entry.1 == 0 {
                        continue;
                    }
                    if entry.0 + entry.1 == base {
                        base = entry.0;
                        len += entry.1;
                        *entry = (0, 0);
                    } else if base + len == entry.0 {
                        len += entry.1;
                        *entry = (0, 0);
                    }
                }
                let mut placed = false;
                for entry in self.free_list.iter_mut() {
                    if entry.1 == 0 {
                        *entry = (base, len);
                        placed = true;
                        break;
                    }
                }
                if !placed {
                    self.leaked = self.leaked.saturating_add(len);
                }
            } else {
                // Partially zeroed: keep the (still dirty) tail parked.
                self.dirty_list[i] = (base + n, len - n);
            }
            return n;
        }
        // Then advance the bump-side frontier, bounded ahead of `next` (no
        // point zeroing the whole 96MB arena eagerly).
        const FRONTIER_AHEAD: usize = 8 * 1024 * 1024;
        if self.zeroed_until < self.next {
            self.zeroed_until = self.next;
        }
        let target = self.limit.min(self.next.saturating_add(FRONTIER_AHEAD));
        let end = target.min(self.zeroed_until.saturating_add(budget));
        if self.zeroed_until >= end {
            return 0;
        }
        let n = end - self.zeroed_until;
        unsafe {
            ptr::write_bytes(self.zeroed_until as *mut u8, 0, n);
        }
        self.zeroed_until = end;
        n
    }

    pub(super) fn allocate(&mut self, len: usize) -> Result<(usize, usize), Error> {
        self.ensure_initialized();
        if len == 0 {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        let aligned = align_len(len).ok_or(Error::Capability(CapError::PermissionDenied))?;
        // Reuse a freed range first (best fit) so one-shot allocations stop
        // growing the bump frontier. Same zeroing guarantee as the bump path.
        let mut best: Option<usize> = None;
        for (index, &(_, entry_len)) in self.free_list.iter().enumerate() {
            if entry_len >= aligned && best.is_none_or(|prev| entry_len < self.free_list[prev].1) {
                best = Some(index);
            }
        }
        if let Some(index) = best {
            let (entry_base, entry_len) = self.free_list[index];
            self.free_list[index] = if entry_len > aligned {
                (entry_base + aligned, entry_len - aligned)
            } else {
                (0, 0)
            };
            // Clean by contract: entries reach free_list only through the
            // idle zeroer (or pre-zeroed bump frontier) — no memset here.
            return Ok((entry_base, aligned));
        }
        let next =
            self.next.checked_add(aligned).ok_or(Error::Capability(CapError::PermissionDenied))?;
        if next > self.limit {
            // Exhaustion was SILENT once and cost a day of bisection
            // (TASK-0076B): a service image allocation failing here kills the
            // spawn with no output anywhere. Say what ran out, with values.
            let pressure = self.stats();
            log_error!(
                "VMO-POOL exhausted: want=0x{:x} used=0x{:x} remaining=0x{:x} peak=0x{:x}",
                aligned,
                pressure.used,
                pressure.remaining,
                pressure.peak_used
            );
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        let base = self.next;
        self.next = next;
        if self.next > self.peak_next {
            self.peak_next = self.next;
        }
        // RFC-0004 (loader + shared-page guard): pages must never leak stale
        // contents. P2 zero-frontier: idle harts pre-zero [next, zeroed_until)
        // via `idle_zero_step`, so we memset only the part the frontier has
        // not covered — the common case is a microsecond no-op instead of the
        // measured 82-90ms BKL hold.
        let end = base + aligned;
        let dirty_from = self.zeroed_until.clamp(base, end);
        if dirty_from < end {
            unsafe {
                ptr::write_bytes(dirty_from as *mut u8, 0, end - dirty_from);
            }
        }
        if self.zeroed_until < end {
            self.zeroed_until = end;
        }
        Ok((base, aligned))
    }

    /// Returns a range handed out by [`allocate`] to the pool (task #124).
    /// Rejects ranges outside the allocated span, unaligned bases, and any
    /// overlap with already-free memory (double-free defense). Coalesces with
    /// the bump frontier and with adjacent free entries; when the bounded
    /// free list is full the range is leaked (counted), never corrupted.
    pub(super) fn free(&mut self, base: usize, len: usize) -> Result<(), Error> {
        self.ensure_initialized();
        let aligned = align_len(len).ok_or(Error::Capability(CapError::PermissionDenied))?;
        if aligned == 0 || base & (PAGE_SIZE - 1) != 0 {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        let end = base.checked_add(aligned).ok_or(Error::Capability(CapError::PermissionDenied))?;
        // Must lie inside the currently allocated span [pool.base, pool.next).
        if base < self.base || end > self.next {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        for &(entry_base, entry_len) in self.free_list.iter().chain(self.dirty_list.iter()) {
            if entry_len != 0 && entry_base < end && base < entry_base + entry_len {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
        }
        // Frontier un-bump: a range touching self.next collapses back into
        // the pool. Its contents are DIRTY, so pull the zero-frontier back —
        // the idle zeroer (or the allocate fallback) re-covers it.
        if base + aligned == self.next {
            self.next = base;
            if self.zeroed_until > base {
                self.zeroed_until = base;
            }
            return Ok(());
        }
        // P2: park in the dirty list; the idle harts zero + promote to
        // free_list (no coalescing across clean/dirty — bounded simplicity;
        // clean-side coalescing happens at promotion).
        for entry in self.dirty_list.iter_mut() {
            if entry.1 == 0 {
                *entry = (base, aligned);
                return Ok(());
            }
        }
        // Dirty list full: zero synchronously (bounded fallback) and place
        // clean, preserving the old behaviour.
        unsafe {
            ptr::write_bytes(base as *mut u8, 0, aligned);
        }
        for entry in self.free_list.iter_mut() {
            if entry.1 == 0 {
                *entry = (base, aligned);
                return Ok(());
            }
        }
        self.leaked = self.leaked.saturating_add(aligned);
        Ok(())
    }

    #[must_use]
    pub(super) fn stats(&self) -> VmoPoolStats {
        let used = self.next.saturating_sub(self.base);
        let remaining = self.limit.saturating_sub(self.next);
        let peak_used = self.peak_next.saturating_sub(self.base);
        VmoPoolStats { base: self.base, used, remaining, peak_used }
    }

    #[allow(dead_code)]
    pub(super) fn contains(&self, addr: usize, len: usize) -> bool {
        if self.base == 0 || len == 0 {
            return false;
        }
        let end = match addr.checked_add(len) {
            Some(end) => end,
            None => return false,
        };
        addr >= self.base && end <= self.limit
    }
}

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
        ctx.address_spaces.map_page(typed.handle, page_va, page_pa, flags)?;
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
