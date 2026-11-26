// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Boot-time proof of the lock-free user copy (TASK-0330 P2, ADR-0102) against a real
//! page table and real frames. A table of its own maps two writable user pages side by side, a
//! read-only user page, a page without the user bit and the console's device window; one
//! address stays unmapped. The copy must land a range that straddles the two writable pages in
//! the two frames the table names and read it back, and must refuse — touching nothing — a
//! write to the read-only page, either direction on the kernel-only page, the unmapped page,
//! the device window and an address in the kernel half.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: user copy ok`, every QEMU lane and the board ladder

use crate::mm::page_table::{PageFlags, PageTable};
use crate::mm::{frame_pool, user_copy, PAGE_SIZE};

const BASE: usize = 0x1000_0000;
const READ_ONLY: usize = BASE + 2 * PAGE_SIZE;
const KERNEL_ONLY: usize = BASE + 3 * PAGE_SIZE;
const UNMAPPED: usize = BASE + 4 * PAGE_SIZE;
const DEVICE: usize = BASE + 5 * PAGE_SIZE;
const USER_RW: PageFlags =
    PageFlags::VALID.union(PageFlags::READ).union(PageFlags::WRITE).union(PageFlags::USER);
const USER_RO: PageFlags = PageFlags::VALID.union(PageFlags::READ).union(PageFlags::USER);
const KERNEL_RW: PageFlags = PageFlags::VALID.union(PageFlags::READ).union(PageFlags::WRITE);

/// The bytes of the frame at `pa`, through the direct map.
fn frame(pa: usize) -> &'static mut [u8] {
    // SAFETY: a frame this selftest took from the pool and gives back at its end; nothing else
    // names it.
    unsafe { core::slice::from_raw_parts_mut(crate::phys::phys_to_virt(pa) as *mut u8, PAGE_SIZE) }
}

fn check(table: &mut PageTable, frames: &[usize; 4]) -> Result<(), &'static str> {
    let [first, second, read_only, kernel_only] = *frames;
    for &pa in frames {
        frame(pa).fill(0);
    }
    table.map(BASE, first, USER_RW).map_err(|_| "map")?;
    table.map(BASE + PAGE_SIZE, second, USER_RW).map_err(|_| "map")?;
    table.map(READ_ONLY, read_only, USER_RO).map_err(|_| "map")?;
    table.map(KERNEL_ONLY, kernel_only, KERNEL_RW).map_err(|_| "map")?;
    let device = crate::hal::platform::uart_window().map(|(base, _)| base & !(PAGE_SIZE - 1));
    if let Some(pa) = device {
        table.map(DEVICE, pa, USER_RW).map_err(|_| "map")?;
    }
    let root = table.root_ppn() * PAGE_SIZE;

    // A range across the two writable pages lands in the two frames the table names.
    let mut pattern = [0u8; 48];
    for (i, byte) in pattern.iter_mut().enumerate() {
        *byte = 0xA0 ^ i as u8;
    }
    let at = BASE + PAGE_SIZE - 16;
    user_copy::copy_out(root, at, &pattern).map_err(|_| "write")?;
    if frame(first)[PAGE_SIZE - 16..] != pattern[..16] || frame(second)[..32] != pattern[16..] {
        return Err("landed");
    }
    let mut back = [0u8; 48];
    user_copy::copy_in(root, at, &mut back).map_err(|_| "read")?;
    if back != pattern {
        return Err("read back");
    }

    // A read-only page is read, never written.
    frame(read_only)[..4].copy_from_slice(b"seal");
    let mut four = [0u8; 4];
    user_copy::copy_in(root, READ_ONLY, &mut four).map_err(|_| "read-only read")?;
    if &four != b"seal" || user_copy::copy_out(root, READ_ONLY, b"open").is_ok() {
        return Err("read-only");
    }
    if &frame(read_only)[..4] != b"seal" {
        return Err("read-only written");
    }

    // A page without the user bit, an unmapped page, a device window, the kernel half: refused
    // in both directions.
    let kernel_half = crate::phys::phys_to_virt(first);
    for (va, name) in [
        (KERNEL_ONLY, "kernel-only"),
        (UNMAPPED, "unmapped"),
        (DEVICE, "device"),
        (kernel_half, "kernel half"),
    ] {
        let mut sink = [0u8; 8];
        if user_copy::copy_in(root, va, &mut sink).is_ok()
            || user_copy::copy_out(root, va, b"refused!").is_ok()
        {
            return Err(name);
        }
    }
    if frame(kernel_only).iter().any(|&b| b != 0) || frame(first)[..8].iter().any(|&b| b != 0) {
        return Err("refusal wrote");
    }
    Ok(())
}

pub(super) fn run() {
    let mut blocks: [Option<crate::frames::Block>; 4] = [None; 4];
    let mut frames = [0usize; 4];
    let mut taken = true;
    for (block, pa) in blocks.iter_mut().zip(frames.iter_mut()) {
        match frame_pool::alloc(0) {
            Ok(b) => {
                *pa = b.base as usize;
                *block = Some(b);
            }
            Err(_) => taken = false,
        }
    }
    let verdict = match (taken, PageTable::new()) {
        (true, Ok(mut table)) => check(&mut table, &frames),
        _ => Err("setup"),
    };
    for block in blocks.into_iter().flatten() {
        frame_pool::free(block);
    }
    match verdict {
        Ok(()) => log_info!(target: "selftest", "KSELFTEST: user copy ok"),
        Err(step) => log_error!(target: "selftest", "KSELFTEST: user copy FAIL ({})", step),
    }
}
