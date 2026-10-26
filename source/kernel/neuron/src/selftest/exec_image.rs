// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel's boot proof of ADR-0080 (TASK-0333 6c-1b). A sealed image of four
//! pages — the header, a read-only page, an executable page, a data page of sixteen bytes with
//! two pages of `.bss` — is loaded into two fresh spaces: both map the SAME frames for the
//! read-only and executable pages, each its OWN frame for the data page (the bytes copied), and
//! the loader counts it so. Then the refusal matrix: a segment off the page grid, two segments
//! on one file page, a seal of a lazy object, of a device-named object, of a writably mapped
//! object, and an exec of an unsealed object through the syscall table.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: exec image shared ok (…)` and `KSELFTEST: exec image reject ok (…)`
//!   on every QEMU lane and the board ladder
//! ADR: docs/adr/0080-a-process-runs-from-a-sealed-image-text-mapped-from-its-frames-data-copied-no-other-exec-path.md

use super::Context;
use crate::cap::{Capability, CapabilityKind, Rights};
use crate::mm::{reclaim, vm_ops, vmo, AddressSpaceManager, AsHandle, PageFlags, PAGE_SIZE};
use crate::syscall::{api, Args, Error as SysError, SyscallTable, SYSCALL_EXEC_VMO};
use crate::task::Pid;
use crate::va_space::RegionKind;

const PAGES: usize = 4;
const RO_OFF: usize = 0x1000;
const RX_OFF: usize = 0x2000;
const RW_OFF: usize = 0x3000;
const RO_VA: usize = 0x10000;
const RX_VA: usize = 0x11000;
const RW_VA: usize = 0x12000;
const RW_FILESZ: usize = 16;
const RW_MEMSZ: usize = 0x3000;
const PF_R: u32 = 4;
const PF_W: u32 = 2;
const PF_X: u32 = 1;

/// One `PT_LOAD` as the image header names it.
#[derive(Clone, Copy)]
struct Seg {
    flags: u32,
    off: usize,
    va: usize,
    filesz: usize,
    memsz: usize,
}

const GOOD: [Seg; 3] = [
    Seg { flags: PF_R, off: RO_OFF, va: RO_VA, filesz: PAGE_SIZE, memsz: PAGE_SIZE },
    Seg { flags: PF_R | PF_X, off: RX_OFF, va: RX_VA, filesz: PAGE_SIZE, memsz: PAGE_SIZE },
    Seg { flags: PF_R | PF_W, off: RW_OFF, va: RW_VA, filesz: RW_FILESZ, memsz: RW_MEMSZ },
];

fn put(out: &mut [u8], at: usize, bytes: &[u8]) {
    out[at..at + bytes.len()].copy_from_slice(bytes);
}

/// An ELF64/RISC-V header with `segs` as its program headers, into page 0.
fn header(segs: &[Seg], out: &mut [u8]) {
    out.fill(0);
    put(out, 0, &[0x7f, b'E', b'L', b'F', 2, 1, 1]);
    put(out, 16, &2u16.to_le_bytes()); // ET_EXEC
    put(out, 18, &243u16.to_le_bytes()); // EM_RISCV
    put(out, 20, &1u32.to_le_bytes());
    put(out, 24, &(RX_VA as u64).to_le_bytes()); // e_entry
    put(out, 32, &64u64.to_le_bytes()); // e_phoff
    put(out, 52, &64u16.to_le_bytes()); // e_ehsize
    put(out, 54, &56u16.to_le_bytes()); // e_phentsize
    put(out, 56, &(segs.len() as u16).to_le_bytes());
    for (i, seg) in segs.iter().enumerate() {
        let at = 64 + 56 * i;
        put(out, at, &1u32.to_le_bytes()); // PT_LOAD
        put(out, at + 4, &seg.flags.to_le_bytes());
        put(out, at + 8, &(seg.off as u64).to_le_bytes());
        put(out, at + 16, &(seg.va as u64).to_le_bytes());
        put(out, at + 24, &(seg.va as u64).to_le_bytes());
        put(out, at + 32, &(seg.filesz as u64).to_le_bytes());
        put(out, at + 40, &(seg.memsz as u64).to_le_bytes());
        put(out, at + 48, &(PAGE_SIZE as u64).to_le_bytes());
    }
}

/// A fresh anonymous image of `PAGES` pages: page 0 the header for `segs`, page 1 all 0x11,
/// page 2 all 0x22, page 3 the bytes 1..=16 then zeros. Not sealed.
fn make_image(segs: &[Seg]) -> Option<u32> {
    let reach = crate::dma_reach::DmaReach::ALL;
    let id = vmo::create(PAGES * PAGE_SIZE, vmo::VmoKind::Anon, &reach).ok()?;
    vmo::zero(id);
    for page in 0..PAGES {
        let pa = vmo::translate(id, page * PAGE_SIZE)?;
        // SAFETY: frames of an object no capability names and no space maps, through the
        // direct map.
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(crate::phys::phys_to_virt(pa) as *mut u8, PAGE_SIZE)
        };
        match page {
            0 => header(segs, bytes),
            1 => bytes.fill(0x11),
            2 => bytes.fill(0x22),
            _ => {
                for (i, b) in bytes[..RW_FILESZ].iter_mut().enumerate() {
                    *b = i as u8 + 1;
                }
            }
        }
    }
    Some(id)
}

/// Load `id` into a fresh space and run its copy plan.
fn load_into(spaces: &mut AddressSpaceManager, id: u32) -> Option<(AsHandle, api::LoadedImage)> {
    let handle = spaces.create().ok()?;
    let mut plan = api::CopyPlan::new();
    match api::load_image(spaces, handle, id, &mut plan) {
        Ok(loaded) => {
            api::run_copy_plan(&plan);
            Some((handle, loaded))
        }
        Err(_) => {
            let _ = spaces.destroy(handle);
            None
        }
    }
}

/// The physical page behind `va` in `handle`.
fn ppn(spaces: &AddressSpaceManager, handle: AsHandle, va: usize) -> Option<usize> {
    spaces.get(handle).ok()?.page_table().lookup(va).map(|pte| pte >> 10)
}

/// Destroy the space and return what the image owned; the lazy `.bss` objects the space alone
/// named go back through the reclaim.
fn tear_down(ctx: &mut Context<'_>, handle: AsHandle, loaded: &api::LoadedImage) {
    let mut ids = alloc::vec::Vec::new();
    if let Ok(space) = ctx.address_spaces.get(handle) {
        space.va_space().vmo_ids(&mut ids);
    }
    let _ = ctx.address_spaces.destroy(handle);
    let _ = api::release_image(&loaded.image);
    reclaim::orphans(ctx.tasks, ctx.address_spaces, &ids, "selftest");
}

pub(super) fn run_exec_image_selftests(ctx: &mut Context<'_>) {
    run_shared(ctx);
    run_rejects(ctx);
    ctx.tasks.set_current(Pid::KERNEL);
}

/// Two spaces from one sealed image: the same frames for text and rodata, their own for data.
fn run_shared(ctx: &mut Context<'_>) {
    let Some(id) = make_image(&GOOD) else {
        log_error!(target: "selftest", "KSELFTEST: exec image shared FAIL (image)");
        return;
    };
    if vmo::seal(id).is_err() {
        log_error!(target: "selftest", "KSELFTEST: exec image shared FAIL (seal)");
        let _ = vmo::destroy(id);
        return;
    }
    let (Some((h1, l1)), Some((h2, l2))) =
        (load_into(ctx.address_spaces, id), load_into(ctx.address_spaces, id))
    else {
        log_error!(target: "selftest", "KSELFTEST: exec image shared FAIL (load)");
        return;
    };
    let spaces = &*ctx.address_spaces;
    let same_ro =
        ppn(spaces, h1, RO_VA).is_some() && ppn(spaces, h1, RO_VA) == ppn(spaces, h2, RO_VA);
    let same_rx =
        ppn(spaces, h1, RX_VA).is_some() && ppn(spaces, h1, RX_VA) == ppn(spaces, h2, RX_VA);
    let own_rw = ppn(spaces, h1, RW_VA).is_some()
        && ppn(spaces, h2, RW_VA).is_some()
        && ppn(spaces, h1, RW_VA) != ppn(spaces, h2, RW_VA);
    let copied = ppn(spaces, h1, RW_VA).is_some_and(|ppn| {
        // SAFETY: a frame the first space owns, zeroed and filled by its copy plan.
        let bytes = unsafe {
            core::slice::from_raw_parts(
                crate::phys::phys_to_virt(ppn << 12) as *const u8,
                RW_FILESZ + 1,
            )
        };
        bytes[..RW_FILESZ].iter().enumerate().all(|(i, &b)| b == i as u8 + 1)
            && bytes[RW_FILESZ] == 0
    });
    let counted = (l1.shared_pages, l1.copied_pages, l2.shared_pages, l2.copied_pages)
        == (2, 1, 2, 1)
        && vm_ops::spaces_mapping_vmo(spaces, id) == 2;
    // The image outlives its first mapper and goes with its last: no capability names it here,
    // so the second space's tear-down reclaims it as an orphan.
    tear_down(ctx, h1, &l1);
    let kept = vm_ops::spaces_mapping_vmo(ctx.address_spaces, id) == 1 && vmo::is_sealed(id);
    tear_down(ctx, h2, &l2);
    let reclaimed = matches!(vmo::destroy(id), Err(vmo::VmoError::NoSuchObject));
    if same_ro && same_rx && own_rw && copied && counted && kept && reclaimed {
        log_info!(target: "selftest", "KSELFTEST: exec image shared ok (text=1 ro=1 copied=1)");
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: exec image shared FAIL (ro={} rx={} rw_own={} copied={} counted={} kept={} reclaimed={})",
            same_ro,
            same_rx,
            own_rw,
            copied,
            counted,
            kept,
            reclaimed
        );
    }
}

/// The refusals, each named: a segment off the page grid, two segments on one file page, a
/// seal of a lazy / device-named / writably mapped object, an exec of an unsealed object.
fn run_rejects(ctx: &mut Context<'_>) {
    let mut unaligned = GOOD;
    unaligned[0].off = RO_OFF + 0x80;
    let mut shared_page = GOOD;
    shared_page[1].off = RO_OFF;
    let mut steps = [false; 6];

    // 1. Off the grid, 2. a shared file page: refused by the loader of a sealed image.
    for (step, segs) in [(0usize, &unaligned), (1, &shared_page)] {
        let Some(id) = make_image(segs) else { break };
        let sealed = vmo::seal(id).is_ok();
        steps[step] = sealed && load_into(ctx.address_spaces, id).is_none();
        if step == 0 {
            // Kept for step 6: a sealed image the loader refuses.
            steps[5] = exec_refuses_unsealed_then_reaches_the_loader(ctx, id);
        }
        let _ = vmo::destroy(id);
    }

    // 3. A lazy object has no seal.
    let reach = crate::dma_reach::DmaReach::ALL;
    if let Ok(lazy) = vmo::create(PAGE_SIZE, vmo::VmoKind::Lazy, &reach) {
        steps[2] = vmo::seal(lazy) == Err(vmo::SealError::Kind);
        let _ = vmo::destroy(lazy);
    }
    // 4. An object a device was told the runs of has no seal.
    if let Ok(named) = vmo::create(PAGE_SIZE, vmo::VmoKind::Anon, &reach) {
        vmo::mark_device_named(named);
        steps[3] = vmo::seal(named) == Err(vmo::SealError::DeviceNamed);
        let _ = vmo::destroy(named);
    }
    // 5. A writably mapped object has no seal until the mapping is gone.
    if let (Ok(id), Ok(handle)) =
        (vmo::create(PAGE_SIZE, vmo::VmoKind::Anon, &reach), ctx.address_spaces.create())
    {
        let flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ | PageFlags::WRITE;
        let runs = vmo::runs(id).unwrap_or_default();
        let mapped =
            ctx.address_spaces.get_mut(handle).ok().and_then(|space| {
                vm_ops::map_runs(space, &runs, flags, RegionKind::Vmo, id, 0).ok()
            });
        let refused = vm_ops::any_space_maps_vmo_writable(ctx.address_spaces, id);
        let unmapped = mapped.is_some_and(|va| {
            ctx.address_spaces
                .get_mut(handle)
                .is_ok_and(|space| vm_ops::unmap_range(space, va, PAGE_SIZE).is_ok())
        });
        let allowed = !vm_ops::any_space_maps_vmo_writable(ctx.address_spaces, id);
        steps[4] = mapped.is_some() && refused && unmapped && allowed && vmo::seal(id).is_ok();
        let _ = ctx.address_spaces.destroy(handle);
        let _ = vmo::destroy(id);
    }

    if steps.iter().all(|&ok| ok) {
        log_info!(
            target: "selftest",
            "KSELFTEST: exec image reject ok (unaligned shared-page lazy device-named writable-mapped unsealed)"
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: exec image reject FAIL (unaligned={} shared_page={} lazy={} device_named={} writable_mapped={} unsealed={})",
            steps[0],
            steps[1],
            steps[2],
            steps[3],
            steps[4],
            steps[5]
        );
    }
}

/// Through the syscall table, as a task: `exec_vmo` of the object in a slot is refused while it
/// is unsealed (PermissionDenied, before any space exists) and, once sealed, reaches the loader
/// — which refuses THIS image for its grid (InvalidArgs): the gates in their order.
fn exec_refuses_unsealed_then_reaches_the_loader(ctx: &mut Context<'_>, id: u32) -> bool {
    const VMO_SLOT: usize = 5;
    ctx.tasks.set_current(Pid::KERNEL);
    let owner = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let Ok(ep) = ctx.router.create_endpoint(1, Some(owner.as_raw())) else { return false };
    let gate = Capability { kind: CapabilityKind::Endpoint(ep), rights: Rights::SEND };
    let image = Capability {
        kind: CapabilityKind::Vmo { id, len: PAGES * PAGE_SIZE },
        rights: Rights::MAP,
    };
    let given = ctx
        .tasks
        .caps_of_mut(owner)
        .is_some_and(|caps| caps.set(0, gate).is_ok() && caps.set(VMO_SLOT, image).is_ok());
    if !given {
        return false;
    }
    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys_ctx = api::Context::new(
        ctx.scheduler,
        ctx.tasks,
        ctx.router,
        ctx.address_spaces,
        timer,
        ctx.hart_timers,
        ctx.waitsets,
        ctx.fences,
    );
    sys_ctx.tasks.set_current(owner);
    let args = Args::new([VMO_SLOT, 1, 0, 0, 0, 0]);
    // The image arrives sealed (step 1 sealed it): unseal is not a thing — so test the order
    // with a second, unsealed object first.
    let reach = crate::dma_reach::DmaReach::ALL;
    let unsealed = vmo::create(PAGE_SIZE, vmo::VmoKind::Anon, &reach).ok();
    let before_seal = unsealed.is_some_and(|raw| {
        let cap = Capability {
            kind: CapabilityKind::Vmo { id: raw, len: PAGE_SIZE },
            rights: Rights::MAP,
        };
        sys_ctx.tasks.caps_of_mut(owner).is_some_and(|caps| caps.set(VMO_SLOT + 1, cap).is_ok())
            && table.dispatch(
                SYSCALL_EXEC_VMO,
                &mut sys_ctx,
                &Args::new([VMO_SLOT + 1, 1, 0, 0, 0, 0]),
            ) == Err(SysError::Capability(crate::cap::CapError::PermissionDenied))
    });
    let at_loader = table.dispatch(SYSCALL_EXEC_VMO, &mut sys_ctx, &args)
        == Err(SysError::AddressSpace(crate::mm::AddressSpaceError::InvalidArgs));
    sys_ctx.tasks.set_current(Pid::KERNEL);
    if let Some(raw) = unsealed {
        let _ = vmo::destroy(raw);
    }
    before_seal && at_loader
}
