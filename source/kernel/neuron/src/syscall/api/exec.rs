// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel's ONE loader (ADR-0080, TASK-0333 6c-1b): `exec_vmo` makes a process from
//! a SEALED image object. Its read-only and executable segments are mapped from the image's own
//! frames — the same frames in every process that runs it — a writable segment's file pages are
//! copied into frames the process owns (staged into the `CopyPlan`, run in phase B with the
//! kernel lock dropped), its `.bss` and the stack are lazy objects (ADR-0077). The
//! pointer-and-length loaders (syscalls 13 and 17) are deleted and their numbers retired.
//! `load_image` is the mapping half, shared with the kernel's boot proof
//! (`selftest/exec_image.rs`); `exec_vmo_phase_a` adds the task half (stack, meta/info pages,
//! identity, QoS).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `KSELFTEST: exec image shared ok` / `exec image reject ok` on every QEMU boot;
//!   every service and app of the ladder; `api/tests.rs` (the capability gate, on host)
//! ADR: docs/adr/0080-a-process-runs-from-a-sealed-image-text-mapped-from-its-frames-data-copied-no-other-exec-path.md

use super::exec_image::{alloc_image, alloc_zeroed_page, map_blocks, map_lazy_fixed, plan_payload};
use super::*;

/// `(vmo_slot, stack_pages, global_pointer, name_ptr, name_len)` — the Decode→Check→Execute
/// discipline.
#[derive(Copy, Clone)]
pub(super) struct ExecVmoArgsTyped {
    vmo_slot: usize,
    stack_pages: usize,
    global_pointer: usize,
    name_ptr: usize,
    name_len: usize,
    /// ADR-0082: `EXEC_FLAG_KILLABLE` — the parent may `task_kill` this process.
    flags: usize,
}

/// ADR-0082: the process may be killed by its parent (and by the kernel's last resort).
pub const EXEC_FLAG_KILLABLE: usize = 1 << 0;
const EXEC_FLAGS_KNOWN: usize = EXEC_FLAG_KILLABLE;

impl ExecVmoArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        let typed = Self {
            vmo_slot: args.get(0),
            stack_pages: args.get(1),
            global_pointer: args.get(2),
            name_ptr: args.get(3),
            name_len: args.get(4),
            flags: args.get(5),
        };
        if typed.stack_pages == 0 || typed.flags & !EXEC_FLAGS_KNOWN != 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(typed)
    }

    #[inline]
    fn check(&self, ctx: &mut Context<'_>) -> Result<(), Error> {
        if self.name_len != 0 {
            const MAX_NAME_LEN: usize = 64;
            if self.name_len > MAX_NAME_LEN {
                return Err(AddressSpaceError::InvalidArgs.into());
            }
            ensure_user_range(ctx, self.name_ptr, self.name_len, UserAccess::Read)?;
        }
        Ok(())
    }
}

/// P2 phased exec: byte-moving work STAGED under the BKL and executed with
/// the BKL dropped (the target task is spawned suspended and its pages are
/// unreachable until the caller resumes it AFTER this syscall returns).
pub(crate) const MAX_COPY_OPS: usize = 64;

#[derive(Clone, Copy)]
pub(crate) struct CopyOp {
    /// Source pointer; `usize::MAX` = pure zero op.
    pub src: usize,
    pub dst: usize,
    pub len: usize,
}

pub(crate) struct CopyPlan {
    pub ops: [CopyOp; MAX_COPY_OPS],
    pub len: usize,
    /// Executable pages were entered: issue `fence.i` after the run.
    pub fence_i: bool,
}

impl CopyPlan {
    pub(crate) const fn new() -> Self {
        Self { ops: [CopyOp { src: 0, dst: 0, len: 0 }; MAX_COPY_OPS], len: 0, fence_i: false }
    }

    pub(super) fn push(&mut self, op: CopyOp) -> Result<(), Error> {
        if self.len >= MAX_COPY_OPS {
            // More pieces than the bounded plan: fail closed (a data segment is one page).
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        self.ops[self.len] = op;
        self.len += 1;
        Ok(())
    }
}

/// Segments an image may carry (lld emits three; the bound keeps the file-page overlap check a
/// scan of a small array).
const MAX_SEGMENTS: usize = 16;

const PT_LOAD: u32 = 1;
const PF_R: u32 = 4;
const PF_W: u32 = 2;
const PF_X: u32 = 1;

/// What `load_image` put into the space.
pub(crate) struct LoadedImage {
    /// `e_entry`.
    pub entry: usize,
    /// The first writable segment's address — the global pointer's base when the caller
    /// names none (`__global_pointer$ = RW + 0x800`, the RISC-V convention).
    pub first_rw_vaddr: Option<usize>,
    /// The end of the highest segment: the stack, meta and info pages lie above it.
    pub max_end_va: usize,
    /// The frames the process owns (its data pages), returned at its teardown.
    pub image: crate::task::ImageAllocs,
    /// Pages mapped from the image's own frames (text, rodata) — shared with every process of
    /// the image.
    pub shared_pages: usize,
    /// Pages copied (a writable segment's file pages).
    pub copied_pages: usize,
}

/// Map the sealed image `vmo` into `as_handle` (ADR-0080): every `PT_LOAD` must begin on a
/// page-aligned file offset at a page-aligned address and share no file page with another;
/// a read-only or executable segment is mapped from the object's frames, a writable one has
/// its file pages copied (planned into `plan`) and its `.bss` tail lazy. On an error nothing
/// of the image stays allocated (the caller destroys the space).
pub(crate) fn load_image(
    spaces: &mut crate::mm::AddressSpaceManager,
    as_handle: crate::mm::AsHandle,
    vmo: u32,
    plan: &mut CopyPlan,
) -> SysResult<LoadedImage> {
    let invalid = || Error::from(AddressSpaceError::InvalidArgs);
    let object_len = crate::mm::vmo::with(vmo, |o| o.len()).ok_or_else(invalid)?;
    let page0 = crate::mm::vmo::translate(vmo, 0).ok_or_else(invalid)?;
    // SAFETY: the first frame of a live object no writer can reach (sealed), through the direct
    // map; the object is at least one page (every object is page-sized).
    let elf: &[u8] =
        unsafe { slice::from_raw_parts(crate::phys::phys_to_virt(page0) as *const u8, PAGE_SIZE) };
    if object_len < PAGE_SIZE || &elf[0..4] != b"\x7FELF" || elf[4] != 2 || elf[5] != 1 {
        return Err(invalid()); // not an ELF64/LE
    }
    let e_entry = read_u64_le(elf, 24)? as usize;
    let e_phoff = read_u64_le(elf, 32)? as usize;
    let e_phentsize = read_u16_le(elf, 54)? as usize;
    let e_phnum = read_u16_le(elf, 56)? as usize;
    // The program headers lie inside the first page (every linker we ship puts them at 0x40).
    let table_end = e_phnum
        .checked_mul(e_phentsize)
        .and_then(|n| n.checked_add(e_phoff))
        .ok_or_else(invalid)?;
    if e_phentsize < 56 || table_end > PAGE_SIZE || e_phnum > MAX_SEGMENTS {
        return Err(invalid());
    }
    let mut loaded = LoadedImage {
        entry: e_entry,
        first_rw_vaddr: None,
        max_end_va: 0,
        image: crate::task::ImageAllocs::new(),
        shared_pages: 0,
        copied_pages: 0,
    };
    // File page ranges taken so far: a file page in two segments would be mapped at two
    // addresses with two permissions.
    let mut file_ranges = [(0usize, 0usize); MAX_SEGMENTS];
    let mut n_ranges = 0usize;
    let result = (|| -> SysResult<()> {
        for i in 0..e_phnum {
            let off = e_phoff + i * e_phentsize;
            if read_u32_le(elf, off)? != PT_LOAD {
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
            if p_filesz > p_memsz || p_offset % PAGE_SIZE != 0 || p_vaddr % PAGE_SIZE != 0 {
                return Err(invalid());
            }
            if p_offset.checked_add(p_filesz).ok_or_else(invalid)? > object_len {
                return Err(invalid());
            }
            let writable = p_flags & PF_W != 0;
            if writable && p_flags & PF_X != 0 {
                return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
            }
            let file_pages =
                if p_filesz == 0 { 0 } else { align_len(p_filesz).ok_or_else(invalid)? };
            let total = align_len(p_memsz).ok_or_else(invalid)?;
            if file_pages != 0 {
                let end = p_offset + file_pages;
                if file_ranges[..n_ranges].iter().any(|&(lo, hi)| p_offset < hi && lo < end) {
                    return Err(invalid());
                }
                file_ranges[n_ranges] = (p_offset, end);
                n_ranges += 1;
            }
            let mut flags = PageFlags::VALID | PageFlags::USER;
            if p_flags & PF_R != 0 {
                flags |= PageFlags::READ;
            }
            if writable {
                flags |= PageFlags::WRITE;
            }
            if p_flags & PF_X != 0 {
                flags |= PageFlags::EXECUTE;
            }
            if !writable {
                // A read-only segment IS its file pages — the image's own frames. A zero tail
                // would be a lazy page, which is never executable (ADR-0077); no linker we
                // ship emits one.
                if file_pages == 0 || total != file_pages {
                    return Err(invalid());
                }
                let runs = vmo_runs_in(vmo, p_offset, file_pages)?;
                let space = spaces.get_mut(as_handle)?;
                crate::mm::vm_ops::map_runs_at(space, p_vaddr, &runs, flags, vmo, p_offset)
                    .map_err(Error::from)?;
                loaded.shared_pages += file_pages / PAGE_SIZE;
                if flags.contains(PageFlags::EXECUTE) {
                    plan.fence_i = true;
                }
            } else {
                if file_pages != 0 {
                    let blocks = alloc_image(file_pages, &mut loaded.image, plan)?;
                    let pieces = crate::mm::vmo::span(vmo, p_offset, p_filesz, false)
                        .map_err(|_| invalid())?;
                    let mut dst_off = 0usize;
                    for (pa, len) in pieces {
                        let pa = pa.ok_or_else(invalid)?;
                        plan_payload(&blocks, dst_off, crate::phys::phys_to_virt(pa), len, plan)?;
                        dst_off += len;
                    }
                    map_blocks(spaces, as_handle, p_vaddr, &blocks, flags)?;
                    loaded.copied_pages += file_pages / PAGE_SIZE;
                }
                if total > file_pages {
                    map_lazy_fixed(
                        spaces,
                        as_handle,
                        p_vaddr + file_pages,
                        total - file_pages,
                        flags,
                    )?;
                }
                if loaded.first_rw_vaddr.is_none() {
                    loaded.first_rw_vaddr = Some(p_vaddr);
                }
            }
            let seg_end = p_vaddr.checked_add(total).ok_or_else(invalid)?;
            loaded.max_end_va = loaded.max_end_va.max(seg_end);
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(loaded),
        Err(err) => {
            // The frames this image took go back now; the space is the caller's to destroy.
            let _ = super::task_image::release_image(&loaded.image);
            Err(err)
        }
    }
}

/// The process's stack: `stack_pages` + 9 head pages + a boundary page, lazy (ADR-0077: a page
/// costs a frame when the stack first reaches it); the guard above stays unmapped.
fn map_process_stack(
    address_spaces: &mut crate::mm::AddressSpaceManager,
    as_handle: crate::mm::AsHandle,
    stack_pages: usize,
) -> SysResult<(VirtAddr, usize)> {
    let total_pages = stack_pages.checked_add(11).ok_or(AddressSpaceError::InvalidArgs)?;
    let stack_len = total_pages.checked_mul(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
    let user_stack_top: usize = 0x2000_0000;
    // Map through the former faulting address (boundary); guard sits above.
    let mapped_top =
        user_stack_top.checked_add(10 * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
    let stack_bottom = mapped_top.checked_sub(stack_len).ok_or(AddressSpaceError::InvalidArgs)?;
    let stack_flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ | PageFlags::WRITE;
    map_lazy_fixed(address_spaces, as_handle, stack_bottom, stack_len, stack_flags)?;
    log_debug!(
        target: "exec",
        "STACK-MAP: va=0x{:x}-0x{:x} lazy pages={} sp=0x{:x}",
        stack_bottom,
        mapped_top.saturating_sub(1),
        total_pages,
        user_stack_top
    );
    // SP one full page below the mapped top (clear of the boundary), 16-aligned.
    let sp_probe = mapped_top.checked_sub(2 * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
    let stack_sp = VirtAddr::new(sp_probe & !0xf).ok_or(AddressSpaceError::InvalidArgs)?;
    Ok((stack_sp, mapped_top))
}

/// `SYSCALL_EXEC_VMO` (63): the unphased fallback (the trap handler routes the live syscall
/// through the phased path: `exec_vmo_phase_a`, then `run_copy_plan` with the BKL dropped).
pub(super) fn sys_exec_vmo(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let mut plan = CopyPlan::new();
    let ret = exec_vmo_phase_a(ctx, args, &mut plan)?;
    run_copy_plan(&plan);
    Ok(ret)
}

/// Phase A of `exec_vmo` (BKL held): the capability gate, the image mapped, the stack, the
/// per-service meta page (the name) and the bootstrap info page, the task spawned Suspended
/// with its kernel-derived identity and born-at-class QoS. The byte moves are staged in `plan`.
pub(crate) fn exec_vmo_phase_a(
    ctx: &mut Context<'_>,
    args: &Args,
    plan: &mut CopyPlan,
) -> SysResult<usize> {
    let typed = ExecVmoArgsTyped::decode(args)?;
    typed.check(ctx)?;

    // Capability gate: only tasks holding a SEND right in slot 0 (bootstrap cap) may exec.
    {
        let _ = ctx
            .tasks
            .current_caps_mut()
            .derive(0, Rights::SEND)
            .map_err(|_| Error::Capability(CapError::PermissionDenied))?;
    }
    let cap = ctx.tasks.current_caps_mut().derive(typed.vmo_slot, Rights::MAP)?;
    let vmo = match cap.kind {
        CapabilityKind::Vmo { id, .. } | CapabilityKind::VmoRo { id, .. } => id,
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    if !crate::mm::vmo::is_sealed(vmo) {
        // ADR-0080: only a sealed object — one no writer can reach — becomes a process.
        log_error!(target: "exec", "EXEC-REFUSED reason=unsealed vmo={}", vmo);
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    // SAFETY: checked in ExecVmoArgsTyped::check (the range is readable user memory).
    let name: &[u8] = if typed.name_len == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(typed.name_ptr as *const u8, typed.name_len) }
    };
    let pid = spawn_sealed(ctx, vmo, typed.stack_pages, typed.global_pointer, name, plan)?;
    // ADR-0082: the parent decides at the spawn whether it may ever kill this process.
    if let Some(task) = ctx.tasks.task_mut(pid) {
        task.set_killable(typed.flags & EXEC_FLAG_KILLABLE != 0);
    }
    Ok(pid.as_index())
}

/// A process from the sealed object `vmo` (ADR-0080): the image mapped, the stack, the
/// per-service meta page (the name) and the bootstrap info page, the task spawned Suspended as
/// the current task's child with its kernel-derived identity and born-at-class QoS. The byte
/// moves are staged in `plan`. The one path every process takes: `exec_vmo` after its
/// capability gate, and the kernel's own spawn of init from the bootfs (ADR-0081).
pub(crate) fn spawn_sealed(
    ctx: &mut Context<'_>,
    vmo: u32,
    stack_pages: usize,
    global_pointer: usize,
    name_bytes: &[u8],
    plan: &mut CopyPlan,
) -> SysResult<task::Pid> {
    if stack_pages == 0 || name_bytes.len() > 64 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let as_handle = ctx.address_spaces.create()?;
    // The frames the child owns (its data pages, meta/info): recorded so success attaches them
    // to the task and failure returns them to the pool.
    let mut image = crate::task::ImageAllocs::new();
    let exec_result = (|| -> SysResult<task::Pid> {
        let loaded = load_image(ctx.address_spaces, as_handle, vmo, plan)?;
        let (entry, first_rw_vaddr, max_end_va) =
            (loaded.entry, loaded.first_rw_vaddr, loaded.max_end_va);
        let (shared_pages, copied_pages) = (loaded.shared_pages, loaded.copied_pages);
        image = loaded.image;

        // The global pointer: the caller's, else the RISC-V convention from the first writable
        // segment, else from the entry.
        const RISCV_GP_BIAS: usize = 0x800;
        let derived_gp = first_rw_vaddr
            .and_then(|vaddr| vaddr.checked_add(RISCV_GP_BIAS))
            .or_else(|| entry.checked_add(RISCV_GP_BIAS))
            .unwrap_or(0);
        let gp = if global_pointer != 0 { global_pointer } else { derived_gp };

        // Stack + guarded top (records into the space).
        let (stack_sp, mapped_top) = map_process_stack(ctx.address_spaces, as_handle, stack_pages)?;

        // Per-service metadata page (RO) + bootstrap info page (RO) at stable addresses just
        // above the mapped stack top: meta = top + 1 page, info = top + 2 pages.
        let meta_va = mapped_top.checked_add(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        let info_va =
            mapped_top.checked_add(2 * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        if info_va + PAGE_SIZE >= USER_VADDR_LIMIT {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        if max_end_va != 0 && (meta_va < max_end_va || info_va < max_end_va) {
            return Err(AddressSpaceError::InvalidArgs.into());
        }

        let meta_pa = alloc_zeroed_page(&mut image)?;
        let mut service_id: u64 = 0;
        if !name_bytes.is_empty() {
            // Kernel-verified service identity token: FNV-1a 64 of the name bytes.
            service_id = 0xcbf29ce484222325u64;
            for &b in name_bytes {
                service_id ^= b as u64;
                service_id = service_id.wrapping_mul(0x100000001b3u64);
            }
            // SAFETY: a fresh zeroed frame the image owns, through the direct map; the name is
            // at most 64 bytes, far below the page.
            unsafe {
                let meta = crate::phys::phys_to_virt(meta_pa);
                ptr::copy_nonoverlapping(name_bytes.as_ptr(), meta as *mut u8, name_bytes.len());
            }
        }
        let ro = PageFlags::VALID | PageFlags::USER | PageFlags::READ;
        ctx.address_spaces.map_page_tracked(as_handle, meta_va, meta_pa, ro)?;

        let info_pa = alloc_zeroed_page(&mut image)?;
        {
            let info = crate::BootstrapInfo {
                version: 2,
                reserved: 0,
                meta_name_ptr: meta_va as u64,
                meta_name_len: name_bytes.len() as u32,
                reserved2: 0,
                service_id,
            };
            // SAFETY: a fresh zeroed frame the image owns; the record is far below a page.
            unsafe {
                ptr::copy_nonoverlapping(
                    &info as *const _ as *const u8,
                    crate::phys::phys_to_virt(info_pa) as *mut u8,
                    core::mem::size_of::<crate::BootstrapInfo>(),
                );
            }
        }
        ctx.address_spaces.map_page_tracked(as_handle, info_va, info_pa, ro)?;

        let entry_pc = VirtAddr::instr_aligned(entry).ok_or(AddressSpaceError::InvalidArgs)?;
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

        if let Some(t) = ctx.tasks.task_mut(pid) {
            // Identity bound to the spawned task (kernel-derived): IPC sender attribution.
            t.set_service_id(service_id);
            // Born-at-class QoS: the display/input path Interactive, background Idle.
            t.set_qos(initial_qos_for(service_id));
            t.set_user_guard_info(task::UserGuardInfo {
                stack_guard_va: mapped_top,
                info_guard_va: Some(guard_va),
            });
        }
        // The record of the exec (ADR-0080): the pages this process shares with every other of
        // its image, the pages it copied, and how many live spaces map the image now.
        log_info!(
            target: "mm",
            "KINIT: exec image (pid={} vmo={} shared_pages={} copied_pages={} mappers={})",
            pid.as_raw(),
            vmo,
            shared_pages,
            copied_pages,
            crate::mm::vm_ops::spaces_mapping_vmo(ctx.address_spaces, vmo)
        );
        // Success: the frames are the task's (teardown returns them). `take` leaves the outer
        // record empty, so the error path below cannot double-free.
        ctx.tasks.set_image_allocs(pid, image.take());
        Ok(pid)
    })();

    if exec_result.is_err() {
        let _ = ctx.address_spaces.destroy(as_handle);
        let _ = super::task_image::release_image(&image);
    }
    exec_result
}
