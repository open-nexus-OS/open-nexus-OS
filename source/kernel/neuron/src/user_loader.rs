// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel user loader bridge for os-lite boot; thin ELF mapping bridge
//! OWNERS: @kernel-team
//! PUBLIC API: load_elf_into_as(bytes, spaces, handle) [temporary]
//! DEPENDS_ON: kernel mm AddressSpaceManager
//! FEATURES: may be active under os-lite bringup only
//! INVARIANTS: Duplicate loader logic must be removed once userspace loader is authoritative
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md
// CRITICAL: Bridge-only. Do not extend functionality here; prefer userspace `nexus-loader`.

extern crate alloc;

use spin::Mutex;

use crate::mm::{AddressSpaceError, AddressSpaceManager, AsHandle, PageFlags, PAGE_SIZE};

const PT_LOAD: u32 = 1;
#[allow(dead_code)]
const PF_X: u32 = 1;
#[allow(dead_code)]
const PF_W: u32 = 2;
#[allow(dead_code)]
const PF_R: u32 = 4;

/// Errors surfaced while loading a user ELF image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum UserLoadError {
    /// Input was shorter than required for the ELF header or program headers.
    Truncated,
    /// ELF magic was not present.
    InvalidElf,
    /// ELF class was not 64-bit.
    WrongClass,
    /// ELF encoding was not little-endian.
    WrongEndian,
    /// Program header table fields were out of range.
    BadPhTable,
    /// Segment bounds were invalid or overflowed.
    SegmentOutOfRange,
    /// Internal page pool ran out of space.
    PoolExhausted,
    /// Mapping the segment failed inside the address-space manager.
    Map(AddressSpaceError),
    /// Chosen user VA window overlaps existing mappings; try a different base.
    MapOverlap,
}

/// Returns (entry_pc, stack_top) on success.
/// Maps the ELF image at its original virtual addresses (no relocation),
/// honoring PT_LOAD p_vaddr/p_flags. The child address space starts empty,
/// so conflicts are not expected.
#[allow(dead_code)]
fn map_at_original_vaddrs(
    bytes: &[u8],
    spaces: &mut AddressSpaceManager,
    handle: AsHandle,
) -> Result<(u64, u64), UserLoadError> {
    // ELF header (we assume little-endian, 64-bit)
    if bytes.len() < 64 { return Err(UserLoadError::Truncated); }
    if &bytes[0..4] != b"\x7FELF" { return Err(UserLoadError::InvalidElf); }
    if bytes[4] != 2 { return Err(UserLoadError::WrongClass); } // ELFCLASS64
    if bytes[5] != 1 { return Err(UserLoadError::WrongEndian); } // little endian
    // Offsets per ELF64
    let e_phoff = le_u64(&bytes[32..40]);
    let e_ehsize = le_u16(&bytes[52..54]);
    let e_phentsize = le_u16(&bytes[54..56]);
    let e_phnum = le_u16(&bytes[56..58]);
    let e_entry = le_u64(&bytes[24..32]);
    if (e_ehsize as usize) > bytes.len() || (e_phoff as usize) >= bytes.len() {
        return Err(UserLoadError::BadPhTable);
    }

    // No relocation: map PT_LOAD segments at p_vaddr.

    // Map each PT_LOAD
    for i in 0..e_phnum {
        let off = e_phoff as usize + (i as usize) * (e_phentsize as usize);
        if off + 56 > bytes.len() { return Err(UserLoadError::Truncated); }
        let p_type = le_u32(&bytes[off..off+4]);
        if p_type != PT_LOAD { continue; }
        let p_flags = le_u32(&bytes[off+4..off+8]);
        let p_offset = le_u64(&bytes[off+8..off+16]) as usize;
        let p_vaddr = le_u64(&bytes[off+16..off+24]) as usize;
        let p_filesz = le_u64(&bytes[off+32..off+40]) as usize;
        let p_memsz = le_u64(&bytes[off+40..off+48]) as usize;

        let mut flags = PageFlags::VALID | PageFlags::USER;
        if p_flags & PF_R != 0 { flags |= PageFlags::READ; }
        if p_flags & PF_W != 0 { flags |= PageFlags::WRITE; }
        if p_flags & PF_X != 0 { flags |= PageFlags::EXECUTE; }

        let seg_hi = p_vaddr.checked_add(p_memsz).ok_or(UserLoadError::SegmentOutOfRange)?;
        let seg_end = align_up(seg_hi, PAGE_SIZE);
        let mut va = align_down(p_vaddr, PAGE_SIZE);
        while va < seg_end {
            // Skip already-mapped VA pages to avoid false overlaps
            let already_mapped = {
                let space_ro = spaces.get(handle).map_err(UserLoadError::Map)?;
                space_ro.page_table().lookup(va).is_some()
            };
            if already_mapped {
                va += PAGE_SIZE;
                continue;
            }
            init_pool_once();
            let pa = PAGE_ALLOC
                .with_lock(|pool| pool.alloc_page())
                .ok_or(UserLoadError::PoolExhausted)?;
            // Zero page
            unsafe {
                core::ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE);
            }
            // Copy file-backed part for this page
            if p_filesz > 0 {
                // Overlap of [va, va+PAGE) with [p_vaddr, p_vaddr+p_filesz)
                let page_off = va.saturating_sub(p_vaddr);
                if page_off < p_filesz {
                    let src_off = p_offset + page_off;
                    let to_copy = core::cmp::min(PAGE_SIZE, p_filesz - page_off);
                    if src_off + to_copy > bytes.len() {
                        return Err(UserLoadError::Truncated);
                    }
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            bytes.as_ptr().add(src_off),
                            pa as *mut u8,
                            to_copy,
                        );
                    }
                }
            }
            if let Err(e) = spaces.map_page(handle, va, pa, flags) {
                if let AddressSpaceError::Mapping(crate::mm::page_table::MapError::Overlap) = e {
                    return Err(UserLoadError::MapOverlap);
                } else {
                    log_error!(target: "userload", "map failed va=0x{:x} pa=0x{:x} flags=0x{:x} err={:?}", va, pa, flags.bits(), e);
                    return Err(UserLoadError::Map(e));
                }
            }
            va += PAGE_SIZE;
        }
    }

    // Map a small RW user stack near 0x4010_0000.
    let stack_pages = 4usize;
    let stack_top = 0x4010_0000usize;
    let mut va = stack_top - stack_pages * PAGE_SIZE;
    for _ in 0..stack_pages {
        init_pool_once();
        let pa = PAGE_ALLOC
            .with_lock(|pool| pool.alloc_page())
            .ok_or(UserLoadError::PoolExhausted)?;
        unsafe { core::ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE); }
        if let Err(e) = spaces.map_page(handle, va, pa, PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::USER) {
            log_error!(target: "userload", "map stack failed va=0x{:x} pa=0x{:x} err={:?}", va, pa, e);
            return Err(UserLoadError::Map(e));
        }
        va += PAGE_SIZE;
    }

    Ok((e_entry, stack_top as u64))
}

/// Choose a non-overlapping user base by probing the target address space.
#[allow(dead_code)]
fn choose_user_base(
    bytes: &[u8],
    spaces: &AddressSpaceManager,
    handle: AsHandle,
    candidates: &[usize],
) -> Option<(usize, usize)> {
    if bytes.len() < 64 { return None; }
    if &bytes[0..4] != b"\x7FELF" { return None; }
    if bytes[4] != 2 || bytes[5] != 1 { return None; }

    let e_phoff = le_u64(&bytes[32..40]) as usize;
    let e_phentsize = le_u16(&bytes[54..56]) as usize;
    let e_phnum = le_u16(&bytes[56..58]) as usize;
    if e_phoff >= bytes.len() { return None; }

    // Compute minimal vaddr across PT_LOAD segments to anchor relocation
    let mut min_vaddr: Option<usize> = None;
    // Collect PT_LOAD segments; use a small fixed-size array to avoid heap deps
    let mut segs: [(usize, usize); 32] = [(0, 0); 32];
    let mut seg_count = 0usize;
    for i in 0..e_phnum {
        let off = e_phoff + i * e_phentsize;
        if off + 56 > bytes.len() { return None; }
        let p_type = le_u32(&bytes[off..off + 4]);
        if p_type != PT_LOAD { continue; }
        let p_vaddr = le_u64(&bytes[off + 16..off + 24]) as usize;
        let p_memsz = le_u64(&bytes[off + 40..off + 48]) as usize;
        let start = align_down(p_vaddr, PAGE_SIZE);
        let end = align_up(p_vaddr.saturating_add(p_memsz), PAGE_SIZE);
        if seg_count < segs.len() {
            segs[seg_count] = (start, end);
            seg_count += 1;
        }
        min_vaddr = Some(match min_vaddr { Some(cur) => cur.min(p_vaddr), None => p_vaddr });
    }
    let base_vaddr = min_vaddr?;
    let space = spaces.get(handle).ok().unwrap();

    'outer: for &base in candidates {
        let delta = base.saturating_sub(align_down(base_vaddr, PAGE_SIZE));
        for idx in 0..seg_count {
            let (seg_start, seg_end) = segs[idx];
            let mut va = align_down(seg_start.saturating_add(delta), PAGE_SIZE);
            let end = align_up(seg_end.saturating_add(delta), PAGE_SIZE);
            while va < end {
                if space.page_table().lookup(va).is_some() {
                    continue 'outer;
                }
                va += PAGE_SIZE;
            }
        }
        // Found a base that doesn't overlap
        return Some((base, delta));
    }
    None
}

#[allow(dead_code)]
fn le_u16(bytes: &[u8]) -> u16 { u16::from_le_bytes([bytes[0], bytes[1]]) }
#[allow(dead_code)]
fn le_u32(bytes: &[u8]) -> u32 { u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) }
#[allow(dead_code)]
fn le_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

#[allow(dead_code)]
fn align_down(addr: usize, align: usize) -> usize { addr & !(align - 1) }
#[allow(dead_code)]
fn align_up(addr: usize, align: usize) -> usize {
    let rem = addr % align;
    if rem == 0 { addr } else { addr + (align - rem) }
}

#[allow(dead_code)]
struct PagePool {
    cursor: usize,
    #[allow(dead_code)]
    base: usize,
    limit: usize,
}

#[allow(dead_code)]
impl PagePool {
    const fn new(base: usize, limit: usize) -> Self { Self { cursor: base, base, limit } }
    fn alloc_page(&mut self) -> Option<usize> {
        if self.cursor + PAGE_SIZE > self.limit { return None; }
        let pa = self.cursor;
        self.cursor += PAGE_SIZE;
        Some(pa)
    }
}

#[allow(dead_code)]
struct LockedPool(Mutex<PagePool>);

#[allow(dead_code)]
impl LockedPool {
    const fn new(base: usize, limit: usize) -> Self { Self(Mutex::new(PagePool::new(base, limit))) }
    fn with_lock<R>(&self, f: impl FnOnce(&mut PagePool) -> R) -> R { f(&mut *self.0.lock()) }
}

#[allow(dead_code)]
fn pool_bounds() -> (usize, usize) {
    // Dedicated kernel PA pool for user pages, identity-mapped by the kernel.
    let base = 0x8060_0000usize;
    (base, base + 512 * 1024)
}

#[allow(dead_code)]
static PAGE_ALLOC: LockedPool = LockedPool::new(0, 0);

#[inline(always)]
#[allow(dead_code)]
fn init_pool_once() {
    // Spin crate may not have 'once' feature; use a simple check and set.
    static mut DONE: bool = false;
    let already;
    unsafe { already = DONE; }
    if !already {
        let (b, l) = pool_bounds();
        *PAGE_ALLOC.0.lock() = PagePool::new(b, l);
        unsafe { DONE = true; }
    }
}

#[inline]
#[allow(dead_code)]
pub fn load_elf_into_as(
    bytes: &[u8],
    spaces: &mut AddressSpaceManager,
    handle: AsHandle,
) -> Result<(u64, u64), UserLoadError> {
    map_at_original_vaddrs(bytes, spaces, handle)
}
