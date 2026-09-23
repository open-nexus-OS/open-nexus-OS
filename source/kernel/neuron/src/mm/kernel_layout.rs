// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The kernel half of every address space (RFC-0098 C4, TASK-0286 P2): the
//! image at its high alias with segment permissions (text RX, the rest RW,
//! the stack guard left out), every memory bank the tree names through the
//! direct map `PHYS_OFFSET + PA` (minus the image's own frames), the console
//! and interrupt-controller windows, and the tree when it lies outside every
//! bank. All GLOBAL, none below `KERNEL_VA_BASE`; the identity map is gone.
//! Split out of `address_space.rs` (structure-gate, RFC-0085 Phase 2). The
//! host build gets the same no-op stub the original had.

use super::page_table::{MapError, PageTable};
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use super::{
    address_space::{align_down, align_up, fence_i, kernel_stack_guard_bytes},
    page_table::{PageFlags, PageTablePage, HUGE_PAGE_SIZE_2M, PAGE_SIZE},
};
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use core::sync::atomic::{AtomicUsize, Ordering};

/// The root page of the FIRST address space (the kernel's): its kernel half is
/// built once and adopted by every later root (TASK-0286 P2b). 0 = not built.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
static KERNEL_ROOT: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(super) fn map_kernel_segments(table: &mut PageTable) -> Result<(), MapError> {
    use crate::phys::{virt_to_phys, PHYS_OFFSET};
    let shared = KERNEL_ROOT.load(Ordering::Acquire);
    if shared != 0 {
        adopt_kernel_half(table, shared as *const PageTablePage);
        return Ok(());
    }
    extern "C" {
        static __text_start: u8;
        static __text_end: u8;
        static __stack_bottom: u8;
        static __image_end: u8;
    }
    const RX: PageFlags =
        PageFlags::VALID.union(PageFlags::READ).union(PageFlags::EXECUTE).union(PageFlags::GLOBAL);
    const RW: PageFlags =
        PageFlags::VALID.union(PageFlags::READ).union(PageFlags::WRITE).union(PageFlags::GLOBAL);
    const RO: PageFlags = PageFlags::VALID.union(PageFlags::READ).union(PageFlags::GLOBAL);

    // The image, at the high alias it runs at: text RX; rodata, data, bss
    // (heap, page tables, the island and selftest stacks) RW; the kernel stack
    // RW above its guard, which stays unmapped so an overflow faults.
    let text_start = align_down(unsafe { &__text_start as *const u8 as usize });
    let text_end = align_up(unsafe { &__text_end as *const u8 as usize });
    let stack_bottom = align_down(unsafe { &__stack_bottom as *const u8 as usize });
    let image_end = align_up(unsafe { &__image_end as *const u8 as usize });
    if text_end <= text_start || image_end <= stack_bottom || stack_bottom < text_end {
        return Err(MapError::OutOfRange);
    }
    map_range(table, text_start, text_end, RX, "TEXT")?;
    fence_i();
    map_range(table, text_end, stack_bottom, RW, "DATA")?;
    let stack_from =
        stack_bottom.checked_add(kernel_stack_guard_bytes()).ok_or(MapError::OutOfRange)?;
    map_range(table, stack_from, image_end, RW, "KSTACK")?;

    // Every bank through the direct map, minus the image's frames (mapped
    // above with their own permissions). RAM the tree does not name is never
    // mapped: nothing kernel-owned can live there.
    let (image_pa, image_pa_end) = (virt_to_phys(text_start), virt_to_phys(image_end));
    let mut tree_in_bank = false;
    let tree = crate::boot_fdt::range();
    for (base, len) in crate::hal::platform::memory_banks() {
        let end = base.checked_add(len).ok_or(MapError::OutOfRange)?;
        if let Some((s, e)) = tree {
            tree_in_bank |= s >= base && e <= end;
        }
        let lo_end = image_pa.clamp(base, end);
        let hi_start = image_pa_end.clamp(base, end);
        map_range(table, base + PHYS_OFFSET, lo_end + PHYS_OFFSET, RW, "BANK")?;
        map_range(table, hi_start + PHYS_OFFSET, end + PHYS_OFFSET, RW, "BANK")?;
    }
    // The console and the interrupt controller: the two device windows the
    // kernel itself drives, at the addresses the device tree named (RFC-0098
    // C3). A platform that failed to initialise has no windows and therefore no
    // console; the harness sees silence.
    for (name, window) in [
        ("UART", crate::hal::platform::uart_window()),
        ("PLIC", crate::hal::platform::plic_window()),
    ] {
        let Some((base, len)) = window else { continue };
        let end = base.checked_add(len).ok_or(MapError::OutOfRange)?;
        map_range(table, align_down(base) + PHYS_OFFSET, align_up(end) + PHYS_OFFSET, RW, name)?;
    }
    // The device tree the firmware handed over in a1 (RFC-0098 C3): inside a
    // bank it is reachable already; anywhere else its pages are mapped
    // read-only (`boot_fdt::report` parses it once the space is active).
    if let (Some((start, end)), false) = (tree, tree_in_bank) {
        map_range(table, start + PHYS_OFFSET, end + PHYS_OFFSET, RO, "DTB")?;
    }
    KERNEL_ROOT.store(table.root.as_ptr() as usize, Ordering::Release);
    log_debug!(target: "mm", "map kernel segments ok");
    Ok(())
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub(super) fn map_kernel_segments(_table: &mut PageTable) -> Result<(), MapError> {
    Ok(())
}

/// Install the kernel half of `src` (root entries 256..512) into `table`'s
/// root: every address space shares the kernel's own second-level tables,
/// built once above (TASK-0286 P2b).
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn adopt_kernel_half(table: &mut PageTable, src: *const PageTablePage) {
    use super::page_table::PT_ENTRIES;
    // SAFETY: both roots are live page-table pages; the kernel half is written
    // once at boot and read-only afterwards.
    unsafe {
        let (dst, src) = (&mut (*table.root.as_ptr()).entries, &(*src).entries);
        dst[PT_ENTRIES / 2..].copy_from_slice(&src[PT_ENTRIES / 2..]);
    }
}

/// Map `[va_start, va_end)` onto the frames behind it (`virt_to_phys`), a
/// 2 MiB leaf wherever both sides are aligned and a whole superpage remains.
/// An overlap names the segment so a boot log says WHAT collided.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn map_range(
    table: &mut PageTable,
    va_start: usize,
    va_end: usize,
    flags: PageFlags,
    what: &str,
) -> Result<(), MapError> {
    let mut va = va_start;
    while va < va_end {
        let pa = crate::phys::virt_to_phys(va);
        let remaining = va_end - va;
        let step = if va % HUGE_PAGE_SIZE_2M == 0 && remaining >= HUGE_PAGE_SIZE_2M {
            table.map_2m(va, pa, flags).inspect_err(|e| overlap(e, what, va))?;
            HUGE_PAGE_SIZE_2M
        } else {
            table.map(va, pa, flags).inspect_err(|e| overlap(e, what, va))?;
            PAGE_SIZE
        };
        va = va.checked_add(step).ok_or(MapError::OutOfRange)?;
    }
    Ok(())
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn overlap(e: &MapError, what: &str, va: usize) {
    if let MapError::Overlap = e {
        log_error!(target: "mm", "AS-MAP: overlap in {} at {:#x}", what, va);
    }
}
