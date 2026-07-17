// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel-side ELF exec loaders split out of the former single-file
//! api.rs: sys_exec and sys_exec_v2 (PT_LOAD mapping with W^X + USER, stack/
//! meta/bootstrap-info pages, service identity + born-at-class QoS), plus
//! their typed decoders.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: neuron host tests + QEMU marker gates (just test-os / ci-os-smp)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::*;

// Typed decoders for seL4-style Decode→Check→Execute

#[derive(Copy, Clone)]
pub(super) struct ExecArgsTyped {
    elf_ptr: usize,
    elf_len: usize,
    stack_pages: usize,
    global_pointer: usize,
}

#[derive(Copy, Clone)]
pub(super) struct ExecV2ArgsTyped {
    elf_ptr: usize,
    elf_len: usize,
    stack_pages: usize,
    global_pointer: usize,
    name_ptr: usize,
    name_len: usize,
}

impl ExecArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        let elf_ptr = args.get(0);
        let elf_len = args.get(1);
        let stack_pages = args.get(2);
        let global_pointer = args.get(3);
        if elf_len == 0 || stack_pages == 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(Self { elf_ptr, elf_len, stack_pages, global_pointer })
    }

    #[inline]
    fn check(&self) -> Result<(), Error> {
        ensure_user_slice(self.elf_ptr, self.elf_len)?;
        if self.stack_pages == 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(())
    }
}

impl ExecV2ArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        let elf_ptr = args.get(0);
        let elf_len = args.get(1);
        let stack_pages = args.get(2);
        let global_pointer = args.get(3);
        let name_ptr = args.get(4);
        let name_len = args.get(5);
        if elf_len == 0 || stack_pages == 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(Self { elf_ptr, elf_len, stack_pages, global_pointer, name_ptr, name_len })
    }

    #[inline]
    fn check(&self) -> Result<(), Error> {
        ensure_user_slice(self.elf_ptr, self.elf_len)?;
        if self.name_len != 0 {
            const MAX_NAME_LEN: usize = 64;
            if self.name_len > MAX_NAME_LEN {
                return Err(AddressSpaceError::InvalidArgs.into());
            }
            ensure_user_slice(self.name_ptr, self.name_len)?;
        }
        Ok(())
    }
}

/// Kernel-side exec loader: parses ELF64/RISC-V, maps PT_LOAD with W^X + USER, sets stack, spawns task.
pub(super) fn sys_exec(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = ExecArgsTyped::decode(args)?;
    typed.check()?;

    // Capability gate: only tasks holding a SEND right in slot 0 (bootstrap cap) may exec.
    {
        let _ = ctx
            .tasks
            .current_caps_mut()
            .derive(0, Rights::SEND)
            .map_err(|_| Error::Capability(CapError::PermissionDenied))?;
    }

    // SAFETY: user slice validated by check; still best-effort.
    let elf = unsafe { slice::from_raw_parts(typed.elf_ptr as *const u8, typed.elf_len) };
    if elf.len() < 64 || &elf[0..4] != b"\x7FELF" {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    if elf[4] != 2 || elf[5] != 1 {
        return Err(AddressSpaceError::InvalidArgs.into()); // not ELF64/LE
    }

    let e_entry = read_u64_le(elf, 24)? as usize;
    let e_phoff = read_u64_le(elf, 32)? as usize;
    let e_phentsize = read_u16_le(elf, 54)? as usize;
    let e_phnum = read_u16_le(elf, 56)? as usize;
    if e_phoff >= elf.len() {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    // RFC-0068: process-exec is a perpetual runtime event, not a bounded boot phase → DEBUG (off by
    // default; recall with `NEXUS_LOG=exec=debug`), so it never clutters the quiet boot overview.
    log_debug!(
        target: "exec",
        "EXEC-ELF hdr entry=0x{:x} phoff=0x{:x} phentsz={} phnum={}",
        e_entry, e_phoff, e_phentsize, e_phnum
    );

    const PT_LOAD: u32 = 1;
    const PF_R: u32 = 4;
    const PF_W: u32 = 2;
    const PF_X: u32 = 1;

    let as_handle = ctx.address_spaces.create()?;

    // Map PT_LOAD segments
    //
    // We also capture the first RW PT_LOAD vaddr so we can derive a sensible
    // RISC-V `gp` when userspace does not provide one. Most RISC-V linkers
    // define `__global_pointer$` as `RW_SEGMENT_VADDR + 0x800`.
    let mut first_rw_vaddr: Option<usize> = None;
    for i in 0..e_phnum {
        let off = e_phoff.checked_add(i * e_phentsize).ok_or(AddressSpaceError::InvalidArgs)?;
        if off + 56 > elf.len() {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        let p_type = read_u32_le(elf, off)?;
        if p_type != PT_LOAD {
            continue;
        }
        let p_flags = read_u32_le(elf, off + 4)?;
        let p_offset = read_u64_le(elf, off + 8)? as usize;
        let p_vaddr = read_u64_le(elf, off + 16)? as usize;
        let p_filesz = read_u64_le(elf, off + 32)? as usize;
        let p_memsz = read_u64_le(elf, off + 40)? as usize;
        {
            // RFC-0068: process-exec is a runtime event → DEBUG (off by default; NEXUS_LOG=exec=debug).
            let first4 = if p_offset + 4 <= elf.len() {
                u32::from_le_bytes([
                    elf[p_offset],
                    elf[p_offset + 1],
                    elf[p_offset + 2],
                    elf[p_offset + 3],
                ])
            } else {
                0
            };
            log_debug!(
                target: "exec",
                "EXEC-ELF phdr load off=0x{:x} vaddr=0x{:x} filesz=0x{:x} memsz=0x{:x} first4=0x{:08x}",
                p_offset, p_vaddr, p_filesz, p_memsz, first4
            );
        }

        if p_memsz == 0 {
            continue;
        }
        if p_filesz > p_memsz {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        let end = p_offset.checked_add(p_filesz).ok_or(AddressSpaceError::InvalidArgs)?;
        if end > elf.len() {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        if (p_flags & PF_W != 0) && (p_flags & PF_X != 0) {
            return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
        }

        // Capture the first RW load segment base for gp derivation (if needed).
        if first_rw_vaddr.is_none() && (p_flags & PF_W != 0) {
            first_rw_vaddr = Some(p_vaddr);
        }

        let page_off = p_vaddr & (PAGE_SIZE - 1);
        let aligned_vaddr = p_vaddr - page_off;
        let alloc_len =
            align_len(p_memsz.checked_add(page_off).ok_or(AddressSpaceError::InvalidArgs)?)
                .ok_or(AddressSpaceError::InvalidArgs)?;
        let (base, alloc_len) = VMO_POOL.lock().allocate(alloc_len)?;

        // Copy file payload
        if p_filesz != 0 {
            unsafe {
                ptr::copy_nonoverlapping(
                    elf.as_ptr().add(p_offset),
                    (base + page_off) as *mut u8,
                    p_filesz,
                );
            }
        }
        // BSS tail is already cleared by the full-allocation zeroing above.

        let mut flags = PageFlags::VALID | PageFlags::USER;
        if p_flags & PF_R != 0 {
            flags |= PageFlags::READ;
        }
        if p_flags & PF_W != 0 {
            flags |= PageFlags::WRITE;
        }
        if p_flags & PF_X != 0 {
            flags |= PageFlags::EXECUTE;
        }

        // Map pages
        let pages = alloc_len / PAGE_SIZE;
        for page in 0..pages {
            let va = aligned_vaddr
                .checked_add(page * PAGE_SIZE)
                .ok_or(AddressSpaceError::InvalidArgs)?;
            let pa = base.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
            ctx.address_spaces.map_page(as_handle, va, pa, flags)?;
        }
    }

    // CRITICAL (RISC-V): Ensure the I-cache sees freshly loaded user text.
    // The kernel just wrote executable bytes into memory. Without `fence.i`, the hart may execute
    // stale instructions at those virtual addresses (especially across AS switches).
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        core::arch::asm!("fence.i", options(nostack));
    }

    // Choose a global pointer for the new task.
    //
    // - Prefer the userspace-provided value (init-lite service table extracts it from ELFs).
    // - Otherwise, derive it from the first RW PT_LOAD segment (common RISC-V convention).
    // - As a last resort, fall back to entry + 0x800 to avoid gp=0 crashes in tiny images.
    const RISCV_GP_BIAS: usize = 0x800;
    let derived_gp = first_rw_vaddr
        .and_then(|vaddr| vaddr.checked_add(RISCV_GP_BIAS))
        .or_else(|| e_entry.checked_add(RISCV_GP_BIAS))
        .unwrap_or(0);
    let gp = if typed.global_pointer != 0 { typed.global_pointer } else { derived_gp };
    {
        let src = if typed.global_pointer != 0 {
            "arg"
        } else if first_rw_vaddr.is_some() {
            "rw+0x800"
        } else {
            "entry+0x800"
        };
        // RFC-0068: process-exec is a runtime event → DEBUG (off by default; NEXUS_LOG=exec=debug).
        log_debug!(target: "exec", "EXEC-ELF gp=0x{:x} src={}", gp, src);
    }

    // Stack
    // Userspace init-lite expects its stack at 0x2000_0000; map downward from there.
    // Map head pages so the top-of-stack address (0x2000_0000) and a boundary page
    // above it are mapped, then seed SP two pages below the mapped top to avoid
    // touching the boundary. Leave a guard page above the boundary.
    let total_pages = typed
        .stack_pages
        .checked_add(11) // requested + 9 head pages + boundary page; guard stays unmapped
        .ok_or(AddressSpaceError::InvalidArgs)?;
    let stack_bytes = total_pages.checked_mul(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
    let (stack_base, stack_len) = VMO_POOL.lock().allocate(stack_bytes)?;
    // Clear the freshly allocated stack to avoid stale data influencing user
    // register setup/prologue logic.
    unsafe {
        ptr::write_bytes(stack_base as *mut u8, 0, stack_len);
    }
    let user_stack_top: usize = 0x2000_0000;
    // Map through the former faulting address (boundary) and leave a guard above.
    let mapped_top =
        user_stack_top.checked_add(10 * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?; // boundary page mapped; guard sits above
    let stack_bottom = mapped_top.checked_sub(stack_len).ok_or(AddressSpaceError::InvalidArgs)?;

    let stack_flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ | PageFlags::WRITE;
    for page in 0..total_pages {
        let va =
            stack_bottom.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        let pa = stack_base.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        ctx.address_spaces.map_page(as_handle, va, pa, stack_flags)?;
    }
    log_debug!(
        target: "exec",
        "STACK-MAP: va=0x{:x}-0x{:x} pa=0x{:x} pages={} sp=0x{:x}",
        stack_bottom,
        mapped_top.saturating_sub(1),
        stack_base,
        total_pages,
        user_stack_top
    );

    let sp_probe = mapped_top.checked_sub(2 * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
    if let Ok(space) = ctx.address_spaces.get(as_handle) {
        let pt = space.page_table();
        let t_sp = pt.translate(sp_probe);
        let t_top_minus_1 = pt.translate(mapped_top.saturating_sub(1));
        let t_top = pt.translate(mapped_top);
        log_debug!(
            target: "exec",
            "STACK-CHECK: base=0x{:x} top=0x{:x} top-1->0x{:x?} top->0x{:x?} sp->0x{:x?}",
            stack_bottom,
            mapped_top,
            t_top_minus_1,
            t_top,
            t_sp
        );
    }

    let entry_pc = VirtAddr::instr_aligned(e_entry).ok_or(AddressSpaceError::InvalidArgs)?;
    // Start SP one full page below the mapped top to stay clear of the boundary, 16-byte aligned.
    let stack_sp_raw = sp_probe & !0xf;
    let stack_sp = VirtAddr::new(stack_sp_raw).ok_or(AddressSpaceError::InvalidArgs)?;
    let bootstrap_slot = SlotIndex::decode(0);

    let parent = ctx.tasks.current_pid();
    let pid = ctx.tasks.spawn(
        parent,
        entry_pc,
        Some(stack_sp),
        Some(as_handle),
        gp,
        bootstrap_slot,
        ctx.scheduler,
        ctx.router,
        ctx.address_spaces,
    )?;

    // RFC-0004 Phase 1 diagnostics: store user guard metadata for trap attribution.
    if let Some(t) = ctx.tasks.task_mut(pid) {
        t.set_user_guard_info(task::UserGuardInfo {
            stack_guard_va: mapped_top,
            info_guard_va: None,
        });
    }
    Ok(pid.as_index())
}

/// Kernel-side exec loader v2: like [`sys_exec`] but also copies the provided service name bytes
/// into a per-service read-only mapping in the child address space (RFC-0004 provenance).
pub(super) fn sys_exec_v2(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = ExecV2ArgsTyped::decode(args)?;
    typed.check()?;

    // Capability gate: only tasks holding a SEND right in slot 0 (bootstrap cap) may exec.
    {
        let _ = ctx
            .tasks
            .current_caps_mut()
            .derive(0, Rights::SEND)
            .map_err(|_| Error::Capability(CapError::PermissionDenied))?;
    }

    // SAFETY: user slice validated by check; still best-effort.
    let elf = unsafe { slice::from_raw_parts(typed.elf_ptr as *const u8, typed.elf_len) };
    if elf.len() < 64 || &elf[0..4] != b"\x7FELF" {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    if elf[4] != 2 || elf[5] != 1 {
        return Err(AddressSpaceError::InvalidArgs.into()); // not ELF64/LE
    }

    let e_entry = read_u64_le(elf, 24)? as usize;
    let e_phoff = read_u64_le(elf, 32)? as usize;
    let e_phentsize = read_u16_le(elf, 54)? as usize;
    let e_phnum = read_u16_le(elf, 56)? as usize;
    if e_phoff >= elf.len() {
        return Err(AddressSpaceError::InvalidArgs.into());
    }

    const PT_LOAD: u32 = 1;
    const PF_R: u32 = 4;
    const PF_W: u32 = 2;
    const PF_X: u32 = 1;

    let as_handle = ctx.address_spaces.create()?;
    let exec_result = (|| -> SysResult<usize> {
        let mut first_rw_vaddr: Option<usize> = None;
        let mut max_end_va: usize = 0;

        // Track mapped PT_LOAD ranges to assert that any existing page-aligned gaps stay unmapped.
        // This is best-effort: we do NOT reject valid ELFs that have no gaps, but we do ensure we
        // never accidentally "inflate" a mapping into a gap.
        #[derive(Clone, Copy)]
        struct LoadRange {
            start: usize,
            end: usize, // page-aligned end
            writable: bool,
        }
        let mut load_ranges: alloc::vec::Vec<LoadRange> = alloc::vec::Vec::new();

        for i in 0..e_phnum {
            let off = e_phoff.checked_add(i * e_phentsize).ok_or(AddressSpaceError::InvalidArgs)?;
            if off + 56 > elf.len() {
                return Err(AddressSpaceError::InvalidArgs.into());
            }
            let p_type = read_u32_le(elf, off)?;
            if p_type != PT_LOAD {
                continue;
            }
            let p_flags = read_u32_le(elf, off + 4)?;
            let p_offset = read_u64_le(elf, off + 8)? as usize;
            let p_vaddr = read_u64_le(elf, off + 16)? as usize;
            let p_filesz = read_u64_le(elf, off + 32)? as usize;
            let p_memsz = read_u64_le(elf, off + 40)? as usize;

            if p_memsz == 0 {
                continue;
            }
            if p_filesz > p_memsz {
                return Err(AddressSpaceError::InvalidArgs.into());
            }
            let end = p_offset.checked_add(p_filesz).ok_or(AddressSpaceError::InvalidArgs)?;
            if end > elf.len() {
                return Err(AddressSpaceError::InvalidArgs.into());
            }
            if (p_flags & PF_W != 0) && (p_flags & PF_X != 0) {
                return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
            }

            if first_rw_vaddr.is_none() && (p_flags & PF_W != 0) {
                first_rw_vaddr = Some(p_vaddr);
            }

            let page_off = p_vaddr & (PAGE_SIZE - 1);
            let aligned_vaddr = p_vaddr - page_off;
            let alloc_len =
                align_len(p_memsz.checked_add(page_off).ok_or(AddressSpaceError::InvalidArgs)?)
                    .ok_or(AddressSpaceError::InvalidArgs)?;
            let (base, alloc_len) = VMO_POOL.lock().allocate(alloc_len)?;

            // Copy file payload (allocation is already zeroed by VmoPool::allocate).
            if p_filesz != 0 {
                unsafe {
                    ptr::copy_nonoverlapping(
                        elf.as_ptr().add(p_offset),
                        (base + page_off) as *mut u8,
                        p_filesz,
                    );
                }
            }

            let mut flags = PageFlags::VALID | PageFlags::USER;
            if p_flags & PF_R != 0 {
                flags |= PageFlags::READ;
            }
            if p_flags & PF_W != 0 {
                flags |= PageFlags::WRITE;
            }
            if p_flags & PF_X != 0 {
                flags |= PageFlags::EXECUTE;
            }

            let pages = alloc_len / PAGE_SIZE;
            for page in 0..pages {
                let va = aligned_vaddr
                    .checked_add(page * PAGE_SIZE)
                    .ok_or(AddressSpaceError::InvalidArgs)?;
                let pa =
                    base.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
                ctx.address_spaces.map_page(as_handle, va, pa, flags)?;
            }

            let seg_end =
                aligned_vaddr.checked_add(alloc_len).ok_or(AddressSpaceError::InvalidArgs)?;
            max_end_va = core::cmp::max(max_end_va, seg_end);

            load_ranges.push(LoadRange {
                start: aligned_vaddr,
                end: seg_end,
                writable: (p_flags & PF_W) != 0,
            });
        }

        // Assert best-effort guard gaps between PT_LOAD mappings (if a gap exists).
        if let Ok(space) = ctx.address_spaces.get(as_handle) {
            load_ranges.sort_by_key(|r| r.start);
            for (idx, r) in load_ranges.iter().enumerate() {
                if !r.writable {
                    continue;
                }
                let next_start = load_ranges.get(idx + 1).map(|n| n.start).unwrap_or(usize::MAX);
                if next_start >= r.end.saturating_add(PAGE_SIZE) && r.end < USER_VADDR_LIMIT {
                    if space.page_table().lookup(r.end).is_some() {
                        panic!("exec_v2: PT_LOAD gap page unexpectedly mapped at 0x{:x}", r.end);
                    }
                }
            }
        }

        // CRITICAL (RISC-V): Ensure the I-cache sees freshly loaded user text.
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        unsafe {
            core::arch::asm!("fence.i", options(nostack));
        }

        // Choose a global pointer for the new task (same policy as sys_exec).
        const RISCV_GP_BIAS: usize = 0x800;
        let derived_gp = first_rw_vaddr
            .and_then(|vaddr| vaddr.checked_add(RISCV_GP_BIAS))
            .or_else(|| e_entry.checked_add(RISCV_GP_BIAS))
            .unwrap_or(0);
        let gp = if typed.global_pointer != 0 { typed.global_pointer } else { derived_gp };

        // Stack (same policy as sys_exec).
        let total_pages =
            typed.stack_pages.checked_add(11).ok_or(AddressSpaceError::InvalidArgs)?;
        let stack_bytes =
            total_pages.checked_mul(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        let (stack_base, stack_len) = VMO_POOL.lock().allocate(stack_bytes)?;
        let user_stack_top: usize = 0x2000_0000;
        let mapped_top =
            user_stack_top.checked_add(10 * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        let stack_bottom =
            mapped_top.checked_sub(stack_len).ok_or(AddressSpaceError::InvalidArgs)?;
        let stack_flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ | PageFlags::WRITE;
        for page in 0..total_pages {
            let va =
                stack_bottom.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
            let pa =
                stack_base.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
            ctx.address_spaces.map_page(as_handle, va, pa, stack_flags)?;
        }

        // Per-service metadata mapping (RO) + bootstrap info page (RO).
        //
        // We intentionally place these at stable addresses just above the mapped stack-top boundary:
        // - mapped_top is `user_stack_top + 10*PAGE_SIZE` (see stack policy)
        // - meta page: mapped_top + 1 page
        // - info page: mapped_top + 2 pages
        //
        // This keeps the contract simple for early userland while remaining provenance-safe.
        let meta_va = mapped_top.checked_add(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        let info_va =
            mapped_top.checked_add(2 * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        if info_va + PAGE_SIZE >= USER_VADDR_LIMIT {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        // Also ensure we don't overlap PT_LOAD segments (defensive).
        if max_end_va != 0 && (meta_va < max_end_va || info_va < max_end_va) {
            return Err(AddressSpaceError::InvalidArgs.into());
        }

        let (meta_pa, _meta_len) = VMO_POOL.lock().allocate(PAGE_SIZE)?;
        let mut service_id: u64 = 0;
        if typed.name_len != 0 {
            // SAFETY: checked in ExecV2ArgsTyped::check.
            let name_bytes =
                unsafe { slice::from_raw_parts(typed.name_ptr as *const u8, typed.name_len) };
            // Kernel-verified service identity token: FNV-1a 64 of the name bytes.
            // This is deterministic, does not allocate, and can be recomputed by userland for display.
            service_id = 0xcbf29ce484222325u64;
            for &b in name_bytes {
                service_id ^= b as u64;
                service_id = service_id.wrapping_mul(0x100000001b3u64);
            }
            unsafe {
                ptr::copy_nonoverlapping(name_bytes.as_ptr(), meta_pa as *mut u8, name_bytes.len());
                if name_bytes.len() < PAGE_SIZE {
                    ptr::write((meta_pa + name_bytes.len()) as *mut u8, 0);
                }
            }
        }
        let meta_flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ;
        ctx.address_spaces.map_page(as_handle, meta_va, meta_pa, meta_flags)?;

        // Bootstrap info page describing the metadata mapping (RO).
        let (info_pa, _info_len) = VMO_POOL.lock().allocate(PAGE_SIZE)?;
        {
            let info = crate::BootstrapInfo {
                version: 2,
                reserved: 0,
                meta_name_ptr: meta_va as u64,
                meta_name_len: typed.name_len as u32,
                reserved2: 0,
                service_id,
            };
            unsafe {
                ptr::copy_nonoverlapping(
                    &info as *const _ as *const u8,
                    info_pa as *mut u8,
                    core::mem::size_of::<crate::BootstrapInfo>(),
                );
            }
        }
        let info_flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ;
        ctx.address_spaces.map_page(as_handle, info_va, info_pa, info_flags)?;

        // Guard page above the bootstrap info page must remain unmapped.
        if let Ok(space) = ctx.address_spaces.get(as_handle) {
            let guard_va = info_va.checked_add(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
            if guard_va < USER_VADDR_LIMIT && space.page_table().lookup(guard_va).is_some() {
                panic!("exec_v2: info guard page mapped at 0x{:x}", guard_va);
            }
        }

        // Proof marker: log the mapping entry (leaf flags must not include WRITE).
        if let Ok(space) = ctx.address_spaces.get(as_handle) {
            if let Some(entry) = space.page_table().lookup(meta_va) {
                use core::fmt::Write as _;
                let writable = (entry & PageFlags::WRITE.bits()) != 0;
                if writable {
                    panic!("exec meta mapping writable");
                }
                // Per-spawn loader detail: off by default (raw write bypasses the diag facade, so
                // honour the same DEBUG/topic gate; `NEXUS_LOG=exec=debug` re-enables). The WRITE
                // safety check above is unconditional — only the trace is gated.
                if crate::log::would_log(crate::log::Level::Debug, "exec") {
                    let mut u = crate::uart::raw_writer();
                    let _ = writeln!(
                        u,
                        "[DEBUG exec] EXEC-META: va=0x{:x} pa=0x{:x} entry=0x{:016x} name_len=0x{:x} info_va=0x{:x}",
                        meta_va, meta_pa, entry, typed.name_len, info_va
                    );
                }
            }
        }

        let entry_pc = VirtAddr::instr_aligned(e_entry).ok_or(AddressSpaceError::InvalidArgs)?;
        let sp_probe =
            mapped_top.checked_sub(2 * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        let stack_sp_raw = sp_probe & !0xf;
        let stack_sp = VirtAddr::new(stack_sp_raw).ok_or(AddressSpaceError::InvalidArgs)?;
        let bootstrap_slot = SlotIndex::decode(0);
        let guard_va = info_va.checked_add(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;

        let parent = ctx.tasks.current_pid();
        let pid = ctx.tasks.spawn(
            parent,
            entry_pc,
            Some(stack_sp),
            Some(as_handle),
            gp,
            bootstrap_slot,
            ctx.scheduler,
            ctx.router,
            ctx.address_spaces,
        )?;

        // Bind identity to the spawned task (kernel-derived): used for IPC sender attribution.
        if let Some(t) = ctx.tasks.task_mut(pid) {
            t.set_service_id(service_id);
            // Born-at-class QoS (production soft-real-time boot): the display/input critical path is
            // created Interactive, background Idle, so the first frame is never starved. See
            // `initial_qos_for`. Escalation is safe here — the kernel sets it at creation, no privilege.
            t.set_qos(initial_qos_for(service_id));
            // RFC-0004 Phase 1 diagnostics: store user guard metadata for trap attribution.
            t.set_user_guard_info(task::UserGuardInfo {
                stack_guard_va: mapped_top,
                info_guard_va: Some(guard_va),
            });
        }

        // Future-facing: once IPC copy-out exists (RFC-0005), we will deliver a BootstrapMsg with
        // `flags::HAS_INFO_PAGE` and `argv_ptr=info_va`. For now, the info/meta pages are at stable
        // addresses and can be read directly by early services.

        Ok(pid.as_index())
    })();

    if exec_result.is_err() {
        let _ = ctx.address_spaces.destroy(as_handle);
    }
    exec_result
}
