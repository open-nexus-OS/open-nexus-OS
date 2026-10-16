// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The MIPS firmware processor's boot (TASK-0329 P1): the firmware heap's map (16 MiB
//! at GPU virtual `0xE1_C000_0000`, MIPS virtual `0xC000_0000`; the top 192 KiB are three fixed
//! 64 KiB slots — connection control, OS init, SYS init; the code region at offset 0, the data
//! region at the private-data section's fixed offset), the boot-data words the boot loader in
//! the firmware reads (stack, the register window's physical address, the page-table pages),
//! the remap register pairs that make the boot code, boot data and exception code reachable
//! before the page table is live, and the firmware page-table entries (one 32-bit entry per
//! 4 KiB page of the heap, four table pages). Goldens: the stock system's live remap and
//! wrapper words (`docs/board/measurements/2026-10-10-gpu-stock-baseline/`).
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below + `tests/gpu_goldens.rs`

use super::heaps;

/// The firmware heap in GPU virtual space.
pub const HEAP_GPU_BASE: u64 = heaps::FIRMWARE_BASE;
/// The heap in the firmware processor's own space.
pub const HEAP_MIPS_BASE: u32 = 0xC000_0000;
pub const HEAP_LOG2_SIZE: u32 = 24;
pub const HEAP_SIZE: u32 = 1 << HEAP_LOG2_SIZE;
pub const HEAP_OFFSET_MASK: u32 = HEAP_SIZE - 1;
/// Kept free at the end of the usable heap (the open driver's reservation).
pub const HEAP_RESERVED: u32 = 1 << 20;
/// The config heap: three 64 KiB slots at the top.
pub const CONFIG_SLOT: u32 = 64 << 10;
pub const CONFIG_HEAP_SIZE: u32 = 3 * CONFIG_SLOT;
pub const CONFIG_OFFSET: u32 = HEAP_SIZE - CONFIG_HEAP_SIZE;
pub const CONNECTION_CTL_OFFSET: u32 = CONFIG_OFFSET;
pub const OSINIT_OFFSET: u32 = CONFIG_OFFSET + CONFIG_SLOT;
pub const SYSINIT_OFFSET: u32 = CONFIG_OFFSET + 2 * CONFIG_SLOT;
/// The allocatable part: below the config heap and the reservation.
pub const HEAP_USABLE: u32 = HEAP_SIZE - CONFIG_HEAP_SIZE - HEAP_RESERVED;

pub const PAGE_SHIFT: u32 = 12;
pub const PAGE: u32 = 1 << PAGE_SHIFT;
/// Table pages: 16 MiB / 4 KiB = 4096 entries × 4 bytes = 4 pages.
pub const PAGE_TABLE_PAGES: usize = 4;
pub const PAGE_TABLE_ENTRIES: usize = (HEAP_SIZE / PAGE) as usize;

/// The firmware's address for a heap offset.
pub const fn fw_addr(heap_offset: u32) -> u32 {
    (heap_offset & HEAP_OFFSET_MASK) | HEAP_MIPS_BASE
}

/// The GPU virtual address for a heap offset.
pub const fn gpu_addr(heap_offset: u32) -> u64 {
    HEAP_GPU_BASE + heap_offset as u64
}

// ---- the page table ---------------------------------------------------------------------

const PTE_PFN_SHIFT: u32 = 6;
/// The physical-page field for a bus wider than 32 bits (ours is 36).
const PTE_PFN_MASK_ABOVE_32BIT: u32 = 0x3FFF_FFC0;
const PTE_CACHE_POLICY_SHIFT: u32 = 3;
const CACHE_POLICY_UNCACHED: u32 = 2;
const PTE_DIRTY: u32 = 1 << 2;
const PTE_VALID: u32 = 1 << 1;
const PTE_GLOBAL: u32 = 1;

/// A firmware page-table entry for `phys`, readable and writable, uncached (every object the
/// firmware shares with the host is device-uncached, as the open driver allocates them).
pub const fn pte_uncached(phys: u64) -> u32 {
    let pfn = ((phys >> PAGE_SHIFT) << PTE_PFN_SHIFT) as u32 & PTE_PFN_MASK_ABOVE_32BIT;
    pfn | (CACHE_POLICY_UNCACHED << PTE_CACHE_POLICY_SHIFT) | PTE_DIRTY | PTE_VALID | PTE_GLOBAL
}

/// Which table page and entry a heap offset's page lives in.
pub const fn pte_slot(heap_offset: u32) -> (usize, usize) {
    let index = ((heap_offset & HEAP_OFFSET_MASK) >> PAGE_SHIFT) as usize;
    (index / (PAGE as usize / 4), index % (PAGE as usize / 4))
}

// ---- the boot data ----------------------------------------------------------------------

/// The boot-data words at the start of the boot-data section (64 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootData {
    /// Physical address of the stack section.
    pub stack_phys: u64,
    /// Physical address of the GPU register window.
    pub reg_base: u64,
    /// Physical addresses of the four page-table pages.
    pub pt_phys: [u64; PAGE_TABLE_PAGES],
}

pub const BOOT_DATA_SIZE: usize = 64;

impl BootData {
    /// The bytes as the boot loader reads them.
    pub fn to_bytes(&self) -> [u8; BOOT_DATA_SIZE] {
        let mut b = [0u8; BOOT_DATA_SIZE];
        b[0..8].copy_from_slice(&self.stack_phys.to_le_bytes());
        b[8..16].copy_from_slice(&self.reg_base.to_le_bytes());
        for (i, p) in self.pt_phys.iter().enumerate() {
            b[16 + i * 8..24 + i * 8].copy_from_slice(&p.to_le_bytes());
        }
        b[48..52].copy_from_slice(&PAGE_SHIFT.to_le_bytes());
        b[52..56].copy_from_slice(&(PAGE_TABLE_PAGES as u32).to_le_bytes());
        b
    }
}

/// The boot-stage word the firmware's boot loader writes inside the boot-data section
/// (`FW_BOOT_*`; the open driver never reads it — we do, as a witness in the FAIL line).
pub const BOOT_STAGE_OFFSET: usize = 0x400;

/// `fw_boot_stage` values.
pub mod boot_stage {
    pub const TLB_INIT_FAILURE: i32 = -2;
    pub const NOT_AVAILABLE: i32 = -1;
    pub const NOT_STARTED: i32 = 0;
    pub const BLDR_STARTED: i32 = 1;
    pub const CACHE_DONE: i32 = 2;
    pub const TLB_DONE: i32 = 3;
    pub const MAIN_STARTED: i32 = 4;
    pub const ALIGNCHECKS_DONE: i32 = 5;
    pub const INIT_DONE: i32 = 6;
}

// ---- the remaps and the wrapper ---------------------------------------------------------

/// The register bank in the firmware processor's space.
pub const REGISTERS_MIPS_BASE: u32 = 0xCF80_0000;
const WRAPPER_REGBANK_ALIGN: u32 = 16;
const WRAPPER_BOOT_ISA_MICROMIPS: u32 = 0x1_0000;

/// `MIPS_WRAPPER_CONFIG` (stock: `0x1CF80`).
pub const fn wrapper_config() -> u32 {
    (REGISTERS_MIPS_BASE >> WRAPPER_REGBANK_ALIGN) | WRAPPER_BOOT_ISA_MICROMIPS
}

/// A remap's input (firmware physical) addresses.
pub const BOOT_REMAP_PHYS_IN: u32 = 0x1FC0_0000;
pub const DATA_REMAP_PHYS_IN: u32 = 0x1FC0_1000;
pub const CODE_REMAP_PHYS_IN: u32 = 0x1FC0_2000;
/// log2 of a remap segment: one page.
const REMAP_LOG2_SEGMENT: u64 = 12;
const REMAP_MODE_ENABLE: u64 = 1;
const REMAP_ADDR_OUT_MASK: u64 = !0xFFFF_FF00_0000_0FFF;

/// A remap register pair: `CONFIG1 = input | enable`, `CONFIG2 = output physical | log2 size`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemapPair {
    pub config1: u64,
    pub config2: u64,
}

pub const fn remap(phys_in: u32, phys_out: u64) -> RemapPair {
    RemapPair {
        config1: phys_in as u64 | REMAP_MODE_ENABLE,
        config2: (phys_out & REMAP_ADDR_OUT_MASK) | REMAP_LOG2_SEGMENT,
    }
}

/// The physical addresses the wrapper needs: where the three boot sections landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootSections {
    pub boot_code_phys: u64,
    pub boot_data_phys: u64,
    pub exception_code_phys: u64,
}

impl BootSections {
    /// Remap 1: the boot code.
    pub const fn remap1(&self) -> RemapPair {
        remap(BOOT_REMAP_PHYS_IN, self.boot_code_phys)
    }
    /// Remap 2: the boot data.
    pub const fn remap2(&self) -> RemapPair {
        remap(DATA_REMAP_PHYS_IN, self.boot_data_phys)
    }
    /// Remap 3: the exception code.
    pub const fn remap3(&self) -> RemapPair {
        remap(CODE_REMAP_PHYS_IN, self.exception_code_phys)
    }
    /// Remap 5 (quirk 63553, a 36-bit bus): input address 0 → the boot code, so an access to
    /// address 0 does not fault.
    pub const fn remap5(&self) -> RemapPair {
        remap(0, self.boot_code_phys)
    }
}

// ---- the heap map -----------------------------------------------------------------------

/// Why a heap allocation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeapError {
    /// Zero bytes, or not a page multiple where one is required.
    Unaligned,
    /// A fixed placement overlaps an existing one or leaves the heap.
    Overlap,
    /// No free run of pages fits.
    Full,
}

/// The heap in pages (16 MiB of 4 KiB).
const HEAP_PAGES: usize = (HEAP_SIZE / PAGE) as usize;
/// The pages first fit hands out: below the reservation and the config heap.
const USABLE_PAGES: usize = (HEAP_USABLE / PAGE) as usize;

/// The heap's map: fixed placements (code at 0, data at the private-data section's offset,
/// the three config slots) and first-fit page runs below the config heap — one bit a page,
/// no table of ranges to run out of (a render context holds about twenty objects, and the
/// 128-range table of 5a held four contexts; TASK-0215 5c-3 P2). Pure.
#[derive(Clone, Debug)]
pub struct HeapMap {
    used: [u64; HEAP_PAGES / 64],
}

impl Default for HeapMap {
    fn default() -> Self {
        Self::new()
    }
}

impl HeapMap {
    /// An empty map with the three config slots reserved.
    pub fn new() -> Self {
        let mut m = Self { used: [0; HEAP_PAGES / 64] };
        for off in [CONNECTION_CTL_OFFSET, OSINIT_OFFSET, SYSINIT_OFFSET] {
            let _ = m.reserve(off, CONFIG_SLOT);
        }
        m
    }

    fn held(&self, page: usize) -> bool {
        (self.used[page / 64] >> (page % 64)) & 1 == 1
    }

    fn mark(&mut self, first: usize, count: usize, held: bool) {
        for p in first..first + count {
            if held {
                self.used[p / 64] |= 1 << (p % 64);
            } else {
                self.used[p / 64] &= !(1 << (p % 64));
            }
        }
    }

    /// Reserves `len` bytes at the fixed `offset` (page-aligned both).
    pub fn reserve(&mut self, offset: u32, len: u32) -> Result<(), HeapError> {
        if len == 0 || offset % PAGE != 0 || len % PAGE != 0 {
            return Err(HeapError::Unaligned);
        }
        if offset as u64 + len as u64 > HEAP_SIZE as u64 {
            return Err(HeapError::Overlap);
        }
        let (first, count) = ((offset / PAGE) as usize, (len / PAGE) as usize);
        if (first..first + count).any(|p| self.held(p)) {
            return Err(HeapError::Overlap);
        }
        self.mark(first, count, true);
        Ok(())
    }

    /// First-fit allocation of `len` bytes (rounded up to pages) below the config heap and
    /// the reservation; returns the heap offset.
    pub fn alloc(&mut self, len: u32) -> Result<u32, HeapError> {
        if len == 0 {
            return Err(HeapError::Unaligned);
        }
        let count = len.div_ceil(PAGE) as usize;
        let mut first = 0;
        while first + count <= USABLE_PAGES {
            // The highest held page of the candidate run pushes the start past it.
            match (first..first + count).rev().find(|&p| self.held(p)) {
                Some(held) => first = held + 1,
                None => {
                    self.mark(first, count, true);
                    return Ok(first as u32 * PAGE);
                }
            }
        }
        Err(HeapError::Full)
    }

    /// Frees the `len` bytes (page-rounded) at `offset`; refused, changing nothing, unless every
    /// page of them is held.
    pub fn free(&mut self, offset: u32, len: u32) -> bool {
        if len == 0 || offset % PAGE != 0 {
            return false;
        }
        let (first, count) = ((offset / PAGE) as usize, len.div_ceil(PAGE) as usize);
        if first + count > HEAP_PAGES || (first..first + count).any(|p| !self.held(p)) {
            return false;
        }
        self.mark(first, count, false);
        true
    }

    /// Bytes held.
    pub fn used(&self) -> u32 {
        self.used.iter().map(|w| w.count_ones()).sum::<u32>() * PAGE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wrapper_and_remap_words_match_the_stock_registers() {
        // Stock: MIPS_WRAPPER_CONFIG 0x1CF80; REMAP1 0x1FC00001 / 0x8979800C with the boot
        // code at 0x89798000; REMAP2 0x1FC01001 / 0x8BDC800C; REMAP3 0x1FC02001 / 0x878F700C;
        // REMAP5 0x1 / 0x8979800C.
        assert_eq!(wrapper_config(), 0x1CF80);
        let s = BootSections {
            boot_code_phys: 0x8979_8000,
            boot_data_phys: 0x8BDC_8000,
            exception_code_phys: 0x878F_7000,
        };
        assert_eq!(s.remap1(), RemapPair { config1: 0x1FC0_0001, config2: 0x8979_800C });
        assert_eq!(s.remap2(), RemapPair { config1: 0x1FC0_1001, config2: 0x8BDC_800C });
        assert_eq!(s.remap3(), RemapPair { config1: 0x1FC0_2001, config2: 0x878F_700C });
        assert_eq!(s.remap5(), RemapPair { config1: 0x1, config2: 0x8979_800C });
    }

    #[test]
    fn a_page_table_entry_carries_the_36_bit_page_uncached() {
        // phys 0x1_2345_6000 → page number 0x12_3456 << 6 = 0x048D_1580 (inside the 30-bit field).
        let e = pte_uncached(0x1_2345_6000);
        assert_eq!(e & 0x3FFF_FFC0, 0x048D_1580);
        assert_eq!(e & 0x3F, 0x17, "uncached | dirty | valid | global");
        assert_eq!(pte_slot(0), (0, 0));
        assert_eq!(pte_slot(0x3FF000), (0, 1023));
        assert_eq!(pte_slot(0x400000), (1, 0));
        assert_eq!(pte_slot(SYSINIT_OFFSET), (3, (SYSINIT_OFFSET as usize >> 12) % 1024));
    }

    #[test]
    fn the_heap_map_keeps_the_config_slots_and_the_fixed_data() {
        assert_eq!(CONFIG_OFFSET, 0xFD_0000);
        assert_eq!(OSINIT_OFFSET, 0xFE_0000);
        assert_eq!(SYSINIT_OFFSET, 0xFF_0000);
        assert_eq!(fw_addr(SYSINIT_OFFSET), 0xC0FF_0000);
        assert_eq!(gpu_addr(0x32000), 0xE1_C003_2000);
        let mut m = HeapMap::new();
        // Code at 0 (0x19000), data at the private-data section's offset 0x32000 (0x5000).
        m.reserve(0, 0x19000).unwrap();
        m.reserve(0x32000, 0x5000).unwrap();
        assert_eq!(m.reserve(0x33000, 0x1000), Err(HeapError::Overlap), "inside the data region");
        assert_eq!(m.reserve(CONFIG_OFFSET, PAGE), Err(HeapError::Overlap), "the config heap");
        // First fit lands right after the code, skips the data, keeps going.
        assert_eq!(m.alloc(0x1000), Ok(0x19000));
        // 0x1A000..0x33000 would overlap the data region: bumped past it.
        assert_eq!(m.alloc(0x19000), Ok(0x37000));
    }

    #[test]
    fn first_fit_bumps_past_every_overlapping_range() {
        let mut m = HeapMap::new();
        m.reserve(0, 0x19000).unwrap();
        m.reserve(0x32000, 0x5000).unwrap();
        // 0x19000 bytes do not fit between 0x19000 and 0x32000 (0x19000 exactly fits: 0x19000+0x19000 = 0x32000).
        assert_eq!(m.alloc(0x19000), Ok(0x19000));
        // The next page goes after the data region.
        assert_eq!(m.alloc(0x1000), Ok(0x37000));
        assert!(m.free(0x19000, 0x19000));
        assert_eq!(m.alloc(0x1000), Ok(0x19000), "a freed range is reused");
        assert_eq!(m.alloc(0), Err(HeapError::Unaligned));
        assert_eq!(m.alloc(HEAP_USABLE), Err(HeapError::Full));
    }

    #[test]
    fn the_map_holds_every_page_and_returns_them() {
        let mut m = HeapMap::new();
        let config = m.used();
        assert_eq!(config, 3 * CONFIG_SLOT);
        // Far more objects than the 128-range table held: every usable page, one by one.
        let pages = HEAP_USABLE / PAGE;
        for i in 0..pages {
            assert_eq!(m.alloc(4), Ok(i * PAGE));
        }
        assert_eq!(m.alloc(4), Err(HeapError::Full), "the reservation stays free");
        assert!(m.free(0x5000, 3 * PAGE));
        assert_eq!(m.alloc(2 * PAGE + 1), Ok(0x5000), "a freed run takes a run that fits");
        assert_eq!(m.alloc(PAGE), Err(HeapError::Full));
        for i in 0..pages {
            assert!(m.free(i * PAGE, 4));
        }
        assert_eq!(m.used(), config, "everything back but the config slots");
    }

    #[test]
    fn test_reject_free_of_a_page_not_held() {
        let mut m = HeapMap::new();
        let at = m.alloc(2 * PAGE).unwrap();
        assert!(!m.free(at + PAGE, 2 * PAGE), "half of it is not held");
        assert!(!m.free(at + 8, PAGE), "not a page start");
        assert!(!m.free(at, 0));
        assert!(m.free(at, 2 * PAGE));
        assert!(!m.free(at, 2 * PAGE), "freed once only");
        assert!(!m.free(HEAP_SIZE - PAGE, 2 * PAGE), "outside the heap");
    }

    #[test]
    fn boot_data_words_are_where_the_boot_loader_reads_them() {
        let b = BootData {
            stack_phys: 0x8000_1000,
            reg_base: 0xCAC0_0000,
            pt_phys: [0x9000_0000, 0x9000_1000, 0x9000_2000, 0x9000_3000],
        }
        .to_bytes();
        assert_eq!(u64::from_le_bytes(b[0..8].try_into().unwrap()), 0x8000_1000);
        assert_eq!(u64::from_le_bytes(b[8..16].try_into().unwrap()), 0xCAC0_0000);
        assert_eq!(u64::from_le_bytes(b[40..48].try_into().unwrap()), 0x9000_3000);
        assert_eq!(u32::from_le_bytes(b[48..52].try_into().unwrap()), 12);
        assert_eq!(u32::from_le_bytes(b[52..56].try_into().unwrap()), 4);
        assert_eq!(&b[56..64], &[0; 8]);
    }
}
