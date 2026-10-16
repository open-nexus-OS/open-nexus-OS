// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The GPU MMU's page tables (TASK-0329 P1): a 40-bit virtual space in three levels —
//! the page catalogue (1024 × 32-bit entries, 1 GiB each), page directories (512 × 64-bit,
//! 2 MiB each; the page size in bits 3..1), page tables (512 × 64-bit for 4 KiB pages; the
//! physical page in bits 39..12, bit 3 SLC bypass, bit 1 read-only, bit 62 PM/FW protect,
//! bit 0 valid). One catalogue is one context's address space; the firmware memory context
//! names its physical address. The encoder is pure over a page allocator trait — table pages
//! are 4 KiB each; the device driver backs them with uncached memory and fences before the
//! flush command; the host backs them with vectors and reads the words back. Every table change
//! accumulates the flush flags the firmware's MMU-cache command needs.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below + `tests/gpu_goldens.rs`
//! INVARIANTS: an entry is written only for a page-aligned address inside 40 bits and a
//!   page-aligned physical page; a mapped page is never mapped again; an unmap of an unmapped
//!   page is refused; the catalogue's physical address never changes for a space's life.

use super::fwif::mmucache;

pub const VA_BITS: u32 = 40;
pub const PAGE_SHIFT: u32 = 12;
pub const PAGE: u64 = 1 << PAGE_SHIFT;
pub const TABLE_PAGE: usize = 4096;
pub const PC_ENTRIES: usize = 1024;
pub const PD_ENTRIES: usize = 512;
pub const PT_ENTRIES: usize = 512;

const PC_INDEX_SHIFT: u32 = 30;
const PD_INDEX_SHIFT: u32 = 21;
const PT_INDEX_SHIFT: u32 = 12;

const PC_VALID: u32 = 1;
const PC_PD_BASE_SHIFT: u32 = 4;
const PC_PD_BASE_ALIGNSHIFT: u32 = 12;
const PD_VALID: u64 = 1;
const PD_PAGE_SIZE_4KB: u64 = 0;
const PD_PT_BASE_MASK: u64 = !0xFFFF_FF00_0000_0FFF;
const PT_VALID: u64 = 1;
const PT_READ_ONLY: u64 = 1 << 1;
const PT_SLC_BYPASS: u64 = 1 << 3;
const PT_PM_FW_PROTECT: u64 = 1 << 62;
const PT_PAGE_MASK: u64 = !0xFFFF_FF00_0000_0FFF;
/// The physical address field: 36 bits of page number above bit 12 (the 40-bit mask).
const PHYS_LIMIT: u64 = 1 << 40;

/// Page flags for a level-0 entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PageFlags {
    pub read_only: bool,
    /// Bypass the system-level cache (firmware-shared memory; CPU-uncached buffers).
    pub slc_bypass: bool,
    /// Protected against the parameter manager / firmware (the open driver sets it on every
    /// firmware object).
    pub pm_fw_protect: bool,
}

/// A page catalogue entry for the directory page at `pd_phys`.
pub const fn pc_entry(pd_phys: u64) -> u32 {
    (((pd_phys >> PC_PD_BASE_ALIGNSHIFT) as u32) << PC_PD_BASE_SHIFT) | PC_VALID
}

/// A page directory entry for the table page at `pt_phys` (4 KiB pages).
pub const fn pd_entry(pt_phys: u64) -> u64 {
    (pt_phys & PD_PT_BASE_MASK) | PD_PAGE_SIZE_4KB | PD_VALID
}

/// A page table entry for the page at `phys`.
pub const fn pt_entry(phys: u64, f: PageFlags) -> u64 {
    let mut e = (phys & PT_PAGE_MASK) | PT_VALID;
    if f.read_only {
        e |= PT_READ_ONLY;
    }
    if f.slc_bypass {
        e |= PT_SLC_BYPASS;
    }
    if f.pm_fw_protect {
        e |= PT_PM_FW_PROTECT;
    }
    e
}

/// The three indices of a virtual address.
pub const fn indices(va: u64) -> (usize, usize, usize) {
    (
        ((va >> PC_INDEX_SHIFT) & (PC_ENTRIES as u64 - 1)) as usize,
        ((va >> PD_INDEX_SHIFT) & (PD_ENTRIES as u64 - 1)) as usize,
        ((va >> PT_INDEX_SHIFT) & (PT_ENTRIES as u64 - 1)) as usize,
    )
}

/// A table page the allocator handed out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TablePage {
    pub handle: u32,
    pub phys: u64,
}

/// Where table pages come from and how their words are written (the device: uncached memory,
/// a fence before the flush; the host: vectors).
pub trait TablePages {
    /// A zeroed 4 KiB page the GPU can read; `None` when none is left.
    fn alloc(&mut self) -> Option<TablePage>;
    fn free(&mut self, page: TablePage);
    fn write_u32(&mut self, page: TablePage, index: usize, value: u32);
    fn write_u64(&mut self, page: TablePage, index: usize, value: u64);
}

/// Why a mapping was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmuError {
    /// The virtual or physical address is not page-aligned.
    Unaligned,
    /// The virtual address lies beyond 40 bits, or the physical beyond the bus.
    OutOfRange,
    /// The page is mapped already.
    Mapped,
    /// The page is not mapped.
    Unmapped,
    /// No table page left.
    NoTablePage,
    /// The space's table of directories/tables is full.
    TableFull,
}

const MAX_PDS: usize = 8;
const MAX_PTS: usize = 64;

#[derive(Clone, Copy, Debug)]
struct Pd {
    pc_index: usize,
    page: TablePage,
    /// Live page-table entries below this directory (its tables' mapped pages).
    mapped: u32,
}

#[derive(Clone, Copy, Debug)]
struct Pt {
    pc_index: usize,
    pd_index: usize,
    page: TablePage,
    mapped: u32,
    /// Which of the 512 entries are live (the host's bookkeeping; the device never reads it).
    live: [u64; PT_ENTRIES / 64],
}

/// A space's slot tables (`MAX_PDS` / `MAX_PTS` entries) on the heap — a fixed array of 64
/// tables was 6.5 KiB of a service's stack frame wherever the space was held (gpud's 32 KiB
/// stack overflowed on the QEMU lane, 2026-10-13) — kept from one space to the next: a device
/// whose heap never frees makes and destroys contexts over the same storage
/// ([`PageTables::destroy`] hands it back).
#[derive(Clone, Debug)]
pub struct TableStorage {
    pds: alloc::vec::Vec<Option<Pd>>,
    pts: alloc::vec::Vec<Option<Pt>>,
}

impl TableStorage {
    pub fn new() -> Self {
        Self { pds: alloc::vec![None; MAX_PDS], pts: alloc::vec![None; MAX_PTS] }
    }
}

impl Default for TableStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// One context's address space: a page catalogue and the directories and tables under it.
#[derive(Clone, Debug)]
pub struct PageTables {
    pc: TablePage,
    pds: alloc::vec::Vec<Option<Pd>>,
    pts: alloc::vec::Vec<Option<Pt>>,
    /// The flush flags accumulated since the last flush was taken.
    flush: u32,
}

impl PageTables {
    /// A new catalogue (one table page) over fresh slot storage.
    pub fn new(pages: &mut impl TablePages) -> Result<Self, MmuError> {
        Self::new_in(TableStorage::new(), pages).map_err(|(e, _)| e)
    }

    /// A new catalogue over `storage`, its slots emptied; the storage comes back with a refusal.
    pub fn new_in(
        storage: TableStorage,
        pages: &mut impl TablePages,
    ) -> Result<Self, (MmuError, TableStorage)> {
        let Some(pc) = pages.alloc() else { return Err((MmuError::NoTablePage, storage)) };
        let TableStorage { mut pds, mut pts } = storage;
        pds.fill(None);
        pts.fill(None);
        Ok(Self { pc, pds, pts, flush: 0 })
    }

    /// The catalogue's physical address — what the firmware memory context names.
    pub const fn root_phys(&self) -> u64 {
        self.pc.phys
    }

    /// The flush flags accumulated since the last take; the caller sends the MMU-cache command
    /// with them and waits for its return slot.
    pub fn take_flush_flags(&mut self) -> u32 {
        core::mem::take(&mut self.flush)
    }

    fn check(va: u64, phys: u64) -> Result<(), MmuError> {
        if va % PAGE != 0 || phys % PAGE != 0 {
            return Err(MmuError::Unaligned);
        }
        if va >= (1u64 << VA_BITS) || phys >= PHYS_LIMIT {
            return Err(MmuError::OutOfRange);
        }
        Ok(())
    }

    fn pd_slot(&self, pc_index: usize) -> Option<usize> {
        self.pds.iter().position(|p| p.is_some_and(|p| p.pc_index == pc_index))
    }

    fn pt_slot(&self, pc_index: usize, pd_index: usize) -> Option<usize> {
        self.pts
            .iter()
            .position(|p| p.is_some_and(|p| p.pc_index == pc_index && p.pd_index == pd_index))
    }

    /// Maps the page at `phys` to `va` with `flags`.
    pub fn map(
        &mut self,
        pages: &mut impl TablePages,
        va: u64,
        phys: u64,
        flags: PageFlags,
    ) -> Result<(), MmuError> {
        Self::check(va, phys)?;
        let (pci, pdi, pti) = indices(va);
        let mut flush = mmucache::LEVEL_0;
        // The directory under the catalogue entry.
        let pd_slot = match self.pd_slot(pci) {
            Some(s) => s,
            None => {
                let s = self.pds.iter().position(|p| p.is_none()).ok_or(MmuError::TableFull)?;
                let page = pages.alloc().ok_or(MmuError::NoTablePage)?;
                pages.write_u32(self.pc, pci, pc_entry(page.phys));
                self.pds[s] = Some(Pd { pc_index: pci, page, mapped: 0 });
                flush |= mmucache::LEVEL_2;
                s
            }
        };
        // The table under the directory entry.
        let pt_slot = match self.pt_slot(pci, pdi) {
            Some(s) => s,
            None => {
                let s = self.pts.iter().position(|p| p.is_none()).ok_or(MmuError::TableFull)?;
                let page = pages.alloc().ok_or(MmuError::NoTablePage)?;
                let Some(pd) = self.pds[pd_slot].as_ref() else { return Err(MmuError::TableFull) };
                pages.write_u64(pd.page, pdi, pd_entry(page.phys));
                self.pts[s] = Some(Pt {
                    pc_index: pci,
                    pd_index: pdi,
                    page,
                    mapped: 0,
                    live: [0; PT_ENTRIES / 64],
                });
                flush |= mmucache::LEVEL_1;
                s
            }
        };
        let Some(pt) = self.pts[pt_slot].as_mut() else { return Err(MmuError::TableFull) };
        if (pt.live[pti / 64] >> (pti % 64)) & 1 == 1 {
            return Err(MmuError::Mapped);
        }
        pages.write_u64(pt.page, pti, pt_entry(phys, flags));
        pt.live[pti / 64] |= 1 << (pti % 64);
        pt.mapped += 1;
        if let Some(pd) = self.pds[pd_slot].as_mut() {
            pd.mapped += 1;
        }
        self.flush |= flush;
        Ok(())
    }

    /// Unmaps the page at `va`; empty tables and directories are freed. The caller MUST send a
    /// waited MMU-cache flush before the page is reused.
    pub fn unmap(&mut self, pages: &mut impl TablePages, va: u64) -> Result<(), MmuError> {
        Self::check(va, 0)?;
        let (pci, pdi, pti) = indices(va);
        let pt_slot = self.pt_slot(pci, pdi).ok_or(MmuError::Unmapped)?;
        let pd_slot = self.pd_slot(pci).ok_or(MmuError::Unmapped)?;
        let Some(pt) = self.pts[pt_slot].as_mut() else { return Err(MmuError::Unmapped) };
        if (pt.live[pti / 64] >> (pti % 64)) & 1 == 0 {
            return Err(MmuError::Unmapped);
        }
        pages.write_u64(pt.page, pti, 0);
        pt.live[pti / 64] &= !(1 << (pti % 64));
        pt.mapped -= 1;
        let mut flush = mmucache::LEVEL_0;
        if pt.mapped == 0 {
            let page = pt.page;
            self.pts[pt_slot] = None;
            if let Some(pd) = self.pds[pd_slot].as_ref() {
                pages.write_u64(pd.page, pdi, 0);
            }
            pages.free(page);
            flush |= mmucache::LEVEL_1;
        }
        if let Some(pd) = self.pds[pd_slot].as_mut() {
            pd.mapped -= 1;
            if pd.mapped == 0 {
                let page = pd.page;
                self.pds[pd_slot] = None;
                pages.write_u32(self.pc, pci, 0);
                pages.free(page);
                flush |= mmucache::LEVEL_2;
            }
        }
        self.flush |= flush;
        Ok(())
    }

    /// Frees every table and the catalogue (the context's destruction) and hands the slot
    /// storage back for the next space. The caller MUST send a waited MMU-cache flush before
    /// the pages are reused.
    pub fn destroy(self, pages: &mut impl TablePages) -> TableStorage {
        let Self { pc, mut pds, mut pts, .. } = self;
        for pt in pts.iter().flatten() {
            pages.free(pt.page);
        }
        for pd in pds.iter().flatten() {
            pages.free(pd.page);
        }
        pages.free(pc);
        pds.fill(None);
        pts.fill(None);
        TableStorage { pds, pts }
    }

    /// Mapped pages.
    pub fn mapped_pages(&self) -> u32 {
        self.pts.iter().flatten().map(|p| p.mapped).sum()
    }

    /// Table pages in use (the catalogue included).
    pub fn table_pages(&self) -> usize {
        1 + self.pds.iter().flatten().count() + self.pts.iter().flatten().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// Pages at 0x9000_0000 upward; the words are kept so the test reads them back.
    struct Pages {
        next: u32,
        freed: Vec<u32>,
        words: Vec<Vec<u64>>,
        limit: u32,
    }

    impl Pages {
        fn new(limit: u32) -> Self {
            Self { next: 0, freed: Vec::new(), words: Vec::new(), limit }
        }
        fn word(&self, page: TablePage, index: usize) -> u64 {
            self.words[page.handle as usize][index]
        }
    }

    impl TablePages for Pages {
        fn alloc(&mut self) -> Option<TablePage> {
            if self.next >= self.limit {
                return None;
            }
            let handle = self.next;
            self.next += 1;
            self.words.push(vec![0u64; 512]);
            Some(TablePage { handle, phys: 0x9000_0000 + handle as u64 * 4096 })
        }
        fn free(&mut self, page: TablePage) {
            self.freed.push(page.handle);
        }
        fn write_u32(&mut self, page: TablePage, index: usize, value: u32) {
            // 1024 × u32 in a 4 KiB page: two per u64 word.
            let w = &mut self.words[page.handle as usize][index / 2];
            if index % 2 == 0 {
                *w = (*w & !0xFFFF_FFFF) | value as u64;
            } else {
                *w = (*w & 0xFFFF_FFFF) | ((value as u64) << 32);
            }
        }
        fn write_u64(&mut self, page: TablePage, index: usize, value: u64) {
            self.words[page.handle as usize][index] = value;
        }
    }

    const UNCACHED: PageFlags =
        PageFlags { read_only: false, slc_bypass: true, pm_fw_protect: true };

    #[test]
    fn the_entries_encode_as_the_format_says() {
        assert_eq!(pc_entry(0x9000_1000), (0x9000_1 << 4) | 1);
        assert_eq!(pd_entry(0x9000_2000), 0x9000_2000 | 1);
        assert_eq!(pt_entry(0x1_2345_6000, UNCACHED), 0x4000_0001_2345_6009);
        assert_eq!(
            pt_entry(0x8000_0000, PageFlags { read_only: true, ..PageFlags::default() }),
            0x8000_0003
        );
        assert_eq!(indices(0x80_0000_0000), (0x200, 0, 0), "the GENERAL heap's first page");
        assert_eq!(indices(0xE1_C003_2000), (0x387, 0, 0x32));
    }

    #[test]
    fn a_map_builds_the_three_levels_and_an_unmap_tears_them_down() {
        let mut pages = Pages::new(8);
        let mut t = PageTables::new(&mut pages).unwrap();
        assert_eq!(t.root_phys(), 0x9000_0000);
        t.map(&mut pages, 0x80_0000_0000, 0x1_0000_0000, UNCACHED).unwrap();
        assert_eq!(t.table_pages(), 3);
        assert_eq!(t.take_flush_flags(), mmucache::LEVEL_2);
        // Catalogue entry 0x200 → the directory page (handle 1), its entry 0 → the table page
        // (handle 2), its entry 0 → the mapped page.
        let pc = TablePage { handle: 0, phys: 0x9000_0000 };
        assert_eq!(pages.word(pc, 0x200 / 2) & 0xFFFF_FFFF, pc_entry(0x9000_1000) as u64);
        assert_eq!(pages.word(TablePage { handle: 1, phys: 0 }, 0), pd_entry(0x9000_2000));
        assert_eq!(
            pages.word(TablePage { handle: 2, phys: 0 }, 0),
            pt_entry(0x1_0000_0000, UNCACHED)
        );
        // A second page in the same table adds no table page and needs a level-0 flush only.
        t.map(&mut pages, 0x80_0000_1000, 0x1_0000_1000, UNCACHED).unwrap();
        assert_eq!(t.table_pages(), 3);
        assert_eq!(t.take_flush_flags(), mmucache::LEVEL_0);
        assert_eq!(t.mapped_pages(), 2);
        t.unmap(&mut pages, 0x80_0000_0000).unwrap();
        assert_eq!(pages.word(TablePage { handle: 2, phys: 0 }, 0), 0);
        assert_eq!(t.take_flush_flags(), mmucache::LEVEL_0);
        t.unmap(&mut pages, 0x80_0000_1000).unwrap();
        assert_eq!(t.table_pages(), 1, "the empty table and directory are freed");
        assert_eq!(pages.freed, vec![2, 1]);
        assert_eq!(pages.word(pc, 0x200 / 2), 0);
        assert_eq!(t.take_flush_flags(), mmucache::LEVEL_2);
    }

    #[test]
    fn a_destroyed_space_returns_every_page_and_its_storage_serves_the_next() {
        let mut pages = Pages::new(16);
        let mut t = PageTables::new(&mut pages).unwrap();
        t.map(&mut pages, 0x80_0000_0000, 0x1_0000_0000, UNCACHED).unwrap();
        t.map(&mut pages, 0xE0_0000_0000, 0x1_0000_1000, UNCACHED).unwrap();
        assert_eq!(t.table_pages(), 5);
        let slots = t.pts.as_ptr();
        let storage = t.destroy(&mut pages);
        let mut freed = pages.freed.clone();
        freed.sort_unstable();
        assert_eq!(freed, vec![0, 1, 2, 3, 4], "the catalogue, two directories, two tables");
        let mut next = PageTables::new_in(storage, &mut pages).unwrap();
        assert_eq!(next.pts.as_ptr(), slots, "the same slot storage, nothing new from the heap");
        assert_eq!((next.table_pages(), next.mapped_pages()), (1, 0), "an empty space");
        next.map(&mut pages, 0x80_0000_0000, 0x1_0000_0000, UNCACHED).unwrap();
        assert_eq!(next.mapped_pages(), 1);
        let mut none = Pages::new(0);
        let (e, back) = PageTables::new_in(TableStorage::new(), &mut none).unwrap_err();
        assert_eq!(e, MmuError::NoTablePage);
        assert_eq!(back.pds.len(), MAX_PDS, "the storage comes back with the refusal");
    }

    #[test]
    fn test_reject_unaligned_va() {
        let mut pages = Pages::new(8);
        let mut t = PageTables::new(&mut pages).unwrap();
        assert_eq!(t.map(&mut pages, 0x80_0000_0800, 0x1000, UNCACHED), Err(MmuError::Unaligned));
        assert_eq!(t.map(&mut pages, 0x80_0000_0000, 0x1800, UNCACHED), Err(MmuError::Unaligned));
        assert_eq!(t.unmap(&mut pages, 0x80_0000_0800), Err(MmuError::Unaligned));
        assert_eq!(t.table_pages(), 1, "nothing was written");
    }

    #[test]
    fn test_reject_va_beyond_40_bits() {
        let mut pages = Pages::new(8);
        let mut t = PageTables::new(&mut pages).unwrap();
        assert_eq!(t.map(&mut pages, 1 << 40, 0x1000, UNCACHED), Err(MmuError::OutOfRange));
        assert_eq!(
            t.map(&mut pages, (1 << 40) - 0x1000, 1 << 40, UNCACHED),
            Err(MmuError::OutOfRange)
        );
        assert_eq!(
            t.map(&mut pages, (1 << 40) - 0x1000, 0x1000, UNCACHED),
            Ok(()),
            "the last page fits"
        );
    }

    #[test]
    fn test_reject_double_map_and_unmap_of_unmapped() {
        let mut pages = Pages::new(8);
        let mut t = PageTables::new(&mut pages).unwrap();
        t.map(&mut pages, 0x1000, 0x2000, UNCACHED).unwrap();
        assert_eq!(t.map(&mut pages, 0x1000, 0x3000, UNCACHED), Err(MmuError::Mapped));
        assert_eq!(t.unmap(&mut pages, 0x2000), Err(MmuError::Unmapped));
        assert_eq!(t.unmap(&mut pages, 0x4000_0000), Err(MmuError::Unmapped), "no directory there");
        assert_eq!(t.mapped_pages(), 1);
    }

    #[test]
    fn test_reject_map_when_no_table_page_is_left() {
        let mut pages = Pages::new(2);
        let mut t = PageTables::new(&mut pages).unwrap();
        // The directory page is taken, the table page is not: refused, the directory stays.
        assert_eq!(t.map(&mut pages, 0x1000, 0x2000, UNCACHED), Err(MmuError::NoTablePage));
        assert_eq!(t.mapped_pages(), 0);
    }
}
