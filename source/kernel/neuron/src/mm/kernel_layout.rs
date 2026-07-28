// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The kernel's identity-map layout: every address space starts with the
//! same GLOBAL kernel segments (text, data, stacks, page pool, the whole
//! VMO arena, UART/PLIC/fw_cfg windows). Split out of `address_space.rs`
//! (structure-gate, RFC-0085 Phase 2). The host build gets the same no-op
//! stub the original had.

use super::address_space::{align_down, align_up, fence_i, kernel_stack_guard_bytes};
use super::page_table::{MapError, PageFlags, PageTable};
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use super::page_table::{HUGE_PAGE_SIZE_2M, PAGE_SIZE};

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(super) fn map_kernel_segments(table: &mut PageTable) -> Result<(), MapError> {
    extern "C" {
        static __text_start: u8;
        static __text_end: u8;
        static __bss_end: u8;
        static __stack_bottom: u8;
        static __stack_top: u8;
        static __selftest_stack_base: u8;
        static __selftest_stack_top: u8;
    }

    let text_start = align_down(unsafe { &__text_start as *const u8 as usize });
    let text_end = align_up(unsafe { &__text_end as *const u8 as usize });
    if text_end <= text_start {
        return Err(MapError::OutOfRange);
    }
    if let Err(e) = map_identity_range(
        table,
        text_start,
        text_end,
        PageFlags::VALID | PageFlags::READ | PageFlags::EXECUTE | PageFlags::GLOBAL,
    ) {
        if let MapError::Overlap = e {
            log_error!(target: "mm", "AS-MAP: overlap in TEXT {:#x}..{:#x}", text_start, text_end);
        }
        return Err(e);
    }
    fence_i();

    let data_start = text_end;
    let data_end = align_up(unsafe { &__bss_end as *const u8 as usize });

    // CRITICAL: Verify HEAP is within mapped range!
    let heap_start = unsafe { core::ptr::addr_of_mut!(crate::HEAP.0) as usize };
    let heap_size = core::mem::size_of::<crate::HeapRegion>();
    let heap_end = heap_start + heap_size;

    if heap_end > data_end {
        log_error!(target: "mm", "AS-MAP: HEAP NOT COVERED! heap={:#x}..{:#x} data_end={:#x}",
            heap_start, heap_end, data_end);
    } else {
        log_debug!(target: "mm", "AS-MAP: HEAP OK in DATA range: heap={:#x}..{:#x} data_end={:#x}",
            heap_start, heap_end, data_end);
    }

    if let Err(e) = map_identity_range(
        table,
        data_start,
        data_end,
        PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
    ) {
        if let MapError::Overlap = e {
            log_error!(target: "mm", "AS-MAP: overlap in DATA {:#x}..{:#x}", data_start, data_end);
        }
        return Err(e);
    }

    let stack_start = align_down(unsafe { &__stack_bottom as *const u8 as usize });
    let stack_end = align_up(unsafe { &__stack_top as *const u8 as usize });
    if stack_end <= stack_start {
        return Err(MapError::OutOfRange);
    }
    let guard_bytes = kernel_stack_guard_bytes();
    let mapped_start = stack_start.checked_add(guard_bytes).ok_or(MapError::OutOfRange)?;
    // DATA/BSS identity range may already cover the low portion of the stack; avoid remapping.
    let map_from = core::cmp::max(mapped_start, data_end);
    log_debug!(
        target: "mm",
        "AS-MAP: KSTACK check: start={:#x} end={:#x} data_end={:#x} guard={} map_from={:#x}",
        stack_start,
        stack_end,
        data_end,
        guard_bytes,
        map_from
    );
    if map_from < stack_end {
        log_debug!(target: "mm", "AS-MAP: mapping KSTACK tail {:#x}..{:#x}", map_from, stack_end);
        if let Err(e) = map_identity_range(
            table,
            map_from,
            stack_end,
            PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
        ) {
            if let MapError::Overlap = e {
                log_error!(
                    target: "mm",
                    "AS-MAP: overlap in KSTACK tail {:#x}..{:#x}",
                    map_from,
                    stack_end
                );
            }
            return Err(e);
        }
        log_debug!(target: "mm", "AS-MAP: KSTACK tail mapped ok");
    } else {
        log_debug!(
            target: "mm",
            "AS-MAP: skip KSTACK mapping; fully covered by DATA {:#x}..{:#x}",
            stack_start,
            stack_end
        );
    }

    let selftest_stack_start = align_down(unsafe { &__selftest_stack_base as *const u8 as usize });
    let selftest_stack_end = align_up(unsafe { &__selftest_stack_top as *const u8 as usize });
    // Map SATP island stack as GLOBAL RW (skip if covered by DATA/BSS identity range)
    extern "C" {
        static __satp_island_stack_base: u8;
        static __satp_island_stack_top: u8;
    }
    let island_start = align_down(unsafe { &__satp_island_stack_base as *const u8 as usize });
    let island_end = align_up(unsafe { &__satp_island_stack_top as *const u8 as usize });
    if island_end > island_start {
        let overlaps_data = island_start < data_end && island_end > data_start;
        if !overlaps_data {
            if let Err(e) = map_identity_range(
                table,
                island_start,
                island_end,
                PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
            ) {
                if let MapError::Overlap = e {
                    log_error!(target: "mm", "AS-MAP: overlap in SATP-ISLAND {:#x}..{:#x}", island_start, island_end);
                }
                return Err(e);
            }
        } else {
            log_debug!(target: "mm", "AS-MAP: skip SATP-ISLAND (covered by DATA) {:#x}..{:#x}", island_start, island_end);
        }
    }
    if selftest_stack_end > selftest_stack_start {
        // Avoid overlapping mappings: if selftest stack lies within [data_start, data_end),
        // it is already covered by the data/BSS identity range.
        let overlaps_data = selftest_stack_start < data_end && selftest_stack_end > data_start;
        if !overlaps_data {
            if let Err(e) = map_identity_range(
                table,
                selftest_stack_start,
                selftest_stack_end,
                PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
            ) {
                if let MapError::Overlap = e {
                    log_error!(target: "mm", "AS-MAP: overlap in SELFTEST {:#x}..{:#x}", selftest_stack_start, selftest_stack_end);
                }
                return Err(e);
            }
        } else {
            log_debug!(target: "mm", "AS-MAP: skip SELFTEST (covered by DATA) {:#x}..{:#x}", selftest_stack_start, selftest_stack_end);
        }
    }

    // Map a page-pool window after BSS so kernel can zero/copy user pages by PA.
    // Keep this in sync with `mm::KERNEL_PAGE_POOL_*` used by early loader/selftest paths.
    let pool_base = super::KERNEL_PAGE_POOL_WINDOW.base;
    let pool_end = super::KERNEL_PAGE_POOL_WINDOW.end();
    if let Err(e) = map_identity_range(
        table,
        pool_base,
        pool_end,
        PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
    ) {
        if let MapError::Overlap = e {
            log_error!(target: "mm", "AS-MAP: overlap in POOL {:#x}..{:#x}", pool_base, pool_end);
        }
        return Err(e);
    }

    // Identity-map the user stack pool used by `task::allocate_guarded_stack` so the kernel can
    // zero freshly allocated stack pages (RFC-0004: no stale bytes / pointer remnants).
    //
    // NOTE: Some early bring-up paths (and page-table allocations depending on layout) may touch
    // low RAM addresses near 0x8000_0000. Map the full 0x8000_0000..0x8020_0000 window to keep
    // these accesses safe and avoid KPGF on unmapped low-RAM.
    let user_stack_pool_base = 0x8000_0000usize;
    let user_stack_pool_end = 0x8000_0000usize + 0x20_0000;
    if let Err(e) = map_identity_range(
        table,
        user_stack_pool_base,
        user_stack_pool_end,
        PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
    ) {
        if let MapError::Overlap = e {
            log_error!(
                target: "mm",
                "AS-MAP: overlap in USER-STACK-POOL {:#x}..{:#x}",
                user_stack_pool_base,
                user_stack_pool_end
            );
        }
        return Err(e);
    }

    // Identity-map the per-service VMO arena at the fixed base used by VMO_POOL.
    let vmo_base = align_up(super::USER_VMO_ARENA_BASE);
    let vmo_end = vmo_base.checked_add(super::USER_VMO_ARENA_LEN).ok_or(MapError::OutOfRange)?;
    if let Err(e) = map_identity_range(
        table,
        vmo_base,
        vmo_end,
        PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
    ) {
        if let MapError::Overlap = e {
            log_error!(
                target: "mm",
                "AS-MAP: overlap in USER-VMO {:#x}..{:#x}",
                vmo_base,
                vmo_end
            );
        }
        return Err(e);
    }

    const UART_BASE: usize = 0x1000_0000;
    const UART_LEN: usize = 0x1000;
    if let Err(e) = map_identity_range(
        table,
        align_down(UART_BASE),
        align_up(UART_BASE + UART_LEN),
        PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
    ) {
        if let MapError::Overlap = e {
            log_error!(target: "mm", "AS-MAP: overlap in UART");
        }
        return Err(e);
    }

    // Identity-map the PLIC so the kernel can claim/complete external device
    // interrupts and route them to userspace drivers (reactive input). QEMU virt
    // places the PLIC at 0x0c00_0000 with a 0x60_0000 register window.
    const PLIC_BASE: usize = 0x0c00_0000;
    const PLIC_LEN: usize = 0x60_0000;
    if let Err(e) = map_identity_range(
        table,
        align_down(PLIC_BASE),
        align_up(PLIC_BASE + PLIC_LEN),
        PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
    ) {
        if let MapError::Overlap = e {
            log_error!(target: "mm", "AS-MAP: overlap in PLIC");
        }
        return Err(e);
    }

    // Identity-map the QEMU virt fw_cfg window (VIRT_FW_CFG = 0x1010_0000, one page) so the kernel
    // can read the boot mode (proof vs interactive) at early boot — the same source selftest-client
    // uses. This gates whether the kernel folds its boot markers into verdicts (interactive) or emits
    // them raw (proof, for verify-uart). Read-only use; mapped RW to match the other device windows.
    const FW_CFG_BASE: usize = 0x1010_0000;
    const FW_CFG_LEN: usize = 0x1000;
    if let Err(e) = map_identity_range(
        table,
        align_down(FW_CFG_BASE),
        align_up(FW_CFG_BASE + FW_CFG_LEN),
        PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
    ) {
        if let MapError::Overlap = e {
            log_error!(target: "mm", "AS-MAP: overlap in fw_cfg");
        }
        return Err(e);
    }

    log_debug!(target: "mm", "map kernel segments ok");
    Ok(())
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub(super) fn map_kernel_segments(_table: &mut PageTable) -> Result<(), MapError> {
    Ok(())
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(super) fn map_identity_range(
    table: &mut PageTable,
    start: usize,
    end: usize,
    flags: PageFlags,
) -> Result<(), MapError> {
    if start >= end {
        return Ok(());
    }
    let mut addr = start;
    while addr < end {
        let remaining = end.checked_sub(addr).ok_or(MapError::OutOfRange)?;
        if addr % HUGE_PAGE_SIZE_2M == 0 && remaining >= HUGE_PAGE_SIZE_2M {
            table.map_2m(addr, addr, flags)?;
            addr = addr.checked_add(HUGE_PAGE_SIZE_2M).ok_or(MapError::OutOfRange)?;
        } else {
            table.map(addr, addr, flags)?;
            addr = addr.checked_add(PAGE_SIZE).ok_or(MapError::OutOfRange)?;
        }
    }
    Ok(())
}
