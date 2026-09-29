// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The byte-level helpers of the embedded init image loader (split out of
//! `selftest/mod.rs` for the structure gate, TASK-0260B P3): a frame for one init page,
//! little-endian ELF field reads, page alignment, one page's slice of a segment copied
//! into its frame, and the word dump behind a mapping check.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the init spawn on every QEMU lane and on the board (`KSELFTEST: segment
//! copied (pages=…)`, `KSELFTEST: init loaded …`)

use super::PAGE_SIZE;

pub(super) fn alloc_init_page() -> Option<usize> {
    // One frame from the pool (TASK-0286 P3b: the fixed page pool is gone).
    // init-lite lives for the whole boot, so its pages are never returned.
    crate::mm::frame_pool::alloc(0).ok().map(|b| b.base as usize)
}

pub(super) fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

pub(super) fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

pub(super) fn read_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

pub(super) fn align_down(addr: usize, align: usize) -> usize {
    addr & !(align - 1)
}

pub(super) fn align_up(addr: usize, align: usize) -> usize {
    let rem = addr % align;
    if rem == 0 {
        addr
    } else {
        addr + (align - rem)
    }
}

pub(super) fn log_symbol_words(
    space: &crate::mm::address_space::AddressSpace,
    virt: usize,
    label: &str,
) {
    use core::fmt::Write as _;

    let page = align_down(virt, PAGE_SIZE);
    let offset = virt - page;

    let mut uart = crate::uart::raw_writer();
    if let Some(entry) = space.page_table().lookup(page) {
        let phys = ((entry >> 10) << 12) + offset;
        unsafe {
            let ptr = crate::phys::phys_to_virt(phys) as *const u32;
            let word0 = core::ptr::read(ptr);
            let word1 = core::ptr::read(ptr.add(1));
            let _ = writeln!(
                uart,
                "[INFO selftest] {} va=0x{:016x} words=0x{:08x} 0x{:08x}",
                label, virt, word0, word1
            );
        }
    } else {
        let _ = writeln!(uart, "[ERROR selftest] {} missing mapping va=0x{:016x}", label, virt);
    }
}

pub(super) fn copy_segment_bytes(
    file: &[u8],
    seg_offset: usize,
    seg_vaddr: usize,
    seg_filesz: usize,
    page_va: usize,
    page_ptr: *mut u8,
) {
    if seg_filesz == 0 {
        return;
    }

    let Some(seg_end) = seg_vaddr.checked_add(seg_filesz) else {
        return;
    };
    let Some(page_end) = page_va.checked_add(PAGE_SIZE) else {
        return;
    };

    let copy_start = core::cmp::max(page_va, seg_vaddr);
    if copy_start >= seg_end {
        return;
    }
    let copy_end = core::cmp::min(page_end, seg_end);
    if copy_end <= copy_start {
        return;
    }

    let len = copy_end - copy_start;
    let src_off = seg_offset + (copy_start - seg_vaddr);
    let dst_off = copy_start - page_va;
    if src_off + len > file.len() {
        return;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(file.as_ptr().add(src_off), page_ptr.add(dst_off), len);
    }
}
