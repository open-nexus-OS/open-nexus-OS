// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The firmware heap's backing (TASK-0329 G1, TASK-0215 5c-3 P2): ONE page-backed DMA VMO made
//! for the GPU's capability (its pages inside the device's reach; `vmo_runs` names them), handed
//! out page by page and taken back — a heap object at a heap offset (fixed: the code and data
//! regions, the three config slots; first-fit: everything else) is backed by one backing page per
//! heap page, and the MIPS page table maps the heap page to the backing page's physical address.
//! The GPU MMU's own table pages and the fault page come from the same pool. What a context held
//! comes back when it goes (a bit a page; a page handed out again is zeroed first — the kernel
//! zeroed each page once, at create). The SoC is non-coherent: a word the CPU wrote is cleaned to
//! memory before the device may read it; a word the device wrote is flushed from the cache before
//! the CPU reads it. Objects never share a page, so a cache line never carries a CPU write and a
//! firmware write at once (the uncached mapping that lifts this is the memory lane's — recorded
//! in the ledger).

use alloc::vec;
use alloc::vec::Vec;

use nexus_abi::{Cap, DmaVmo};
use nexus_gfx::backend::gpu::mips_boot::{self, HeapMap, HEAP_SIZE, PAGE};

/// Bytes of one page.
const PAGE_BYTES: usize = PAGE as usize;
/// A heap page without a backing page.
const UNBACKED: u16 = u16::MAX;

/// An object in the firmware heap: `len` bytes at heap offset `off`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HeapObj {
    pub off: u32,
    pub len: u32,
}

impl HeapObj {
    /// The firmware's address of the object (its first byte).
    pub const fn fw_addr(&self) -> u32 {
        mips_boot::fw_addr(self.off)
    }
}

/// One backing page: its index in the VMO and its physical (bus) address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Page {
    pub index: usize,
    pub phys: u64,
}

pub(super) struct FwMemory {
    vmo: DmaVmo,
    /// The cache block the harts maintain in; 0 = coherent (nothing to maintain).
    block: usize,
    /// Physical address of every backing page, by index.
    phys: Vec<u64>,
    /// Backing pages handed out, a bit each.
    held: Vec<u64>,
    /// Backing pages handed out at least once: zeroed before they are handed out again.
    used_once: Vec<u64>,
    heap: HeapMap,
    /// Heap page → backing page (`UNBACKED` where none).
    map: Vec<u16>,
}

fn bit(words: &[u64], i: usize) -> bool {
    (words[i / 64] >> (i % 64)) & 1 == 1
}

fn set_bit(words: &mut [u64], i: usize, on: bool) {
    if on {
        words[i / 64] |= 1 << (i % 64);
    } else {
        words[i / 64] &= !(1 << (i % 64));
    }
}

impl FwMemory {
    /// `pages` backing pages for the device behind `device`; `block` is the device's cache
    /// block (0 for a coherent one).
    pub(super) fn new(device: Cap, pages: usize, block: usize) -> Result<Self, &'static str> {
        if pages >= usize::from(UNBACKED) {
            return Err("backing pages beyond the heap map's index");
        }
        let vmo = DmaVmo::anonymous(device, pages * PAGE_BYTES).map_err(|_| "dma vmo")?;
        let mut phys = Vec::with_capacity(pages);
        for run in vmo.runs() {
            let n = (run.len / PAGE as u64) as usize;
            for i in 0..n {
                phys.push(run.bus + (i as u64) * PAGE as u64);
            }
        }
        if phys.len() < pages {
            return Err("dma runs short");
        }
        phys.truncate(pages);
        let words = pages.div_ceil(64);
        Ok(Self {
            vmo,
            block,
            phys,
            held: vec![0; words],
            used_once: vec![0; words],
            heap: HeapMap::new(),
            map: vec![UNBACKED; (HEAP_SIZE / PAGE) as usize],
        })
    }

    pub(super) const fn block(&self) -> usize {
        self.block
    }

    /// Backing pages free.
    pub(super) fn free_pages(&self) -> usize {
        (0..self.phys.len()).filter(|&i| !bit(&self.held, i)).count()
    }

    /// One zeroed backing page.
    pub(super) fn alloc_page(&mut self) -> Option<Page> {
        let index = (0..self.phys.len()).find(|&i| !bit(&self.held, i))?;
        set_bit(&mut self.held, index, true);
        let page = Page { index, phys: self.phys[index] };
        if bit(&self.used_once, index) {
            self.page_bytes_mut(&page).fill(0);
            self.clean_page(&page);
        }
        set_bit(&mut self.used_once, index, true);
        Some(page)
    }

    /// Gives a backing page back (a GPU table page; a heap object's pages go with [`Self::free`]).
    pub(super) fn free_page(&mut self, page: Page) {
        if page.index < self.phys.len() {
            set_bit(&mut self.held, page.index, false);
        }
    }

    fn back(&mut self, obj: HeapObj) -> Option<HeapObj> {
        let first = (obj.off / PAGE) as usize;
        let count = obj.len.div_ceil(PAGE) as usize;
        for i in 0..count {
            let Some(page) = self.alloc_page() else {
                // Undo: the pages backed so far and the heap range.
                for j in 0..i {
                    let index = usize::from(self.map[first + j]);
                    self.free_page(Page { index, phys: self.phys[index] });
                    self.map[first + j] = UNBACKED;
                }
                self.heap.free(obj.off, obj.len);
                return None;
            };
            self.map[first + i] = page.index as u16;
        }
        Some(obj)
    }

    /// Reserve `len` bytes at the fixed heap `off` (page-aligned both), backed.
    pub(super) fn reserve(&mut self, off: u32, len: u32) -> Option<HeapObj> {
        self.heap.reserve(off, len).ok()?;
        self.back(HeapObj { off, len })
    }

    /// Back one of the three config slots the heap map reserved at creation (connection
    /// control, OS init, SYS init): one page at the slot's offset, no second reservation.
    pub(super) fn back_config_slot(&mut self, off: u32) -> Option<HeapObj> {
        let first = (off / PAGE) as usize;
        let page = self.alloc_page()?;
        self.map[first] = page.index as u16;
        Some(HeapObj { off, len: PAGE })
    }

    /// Allocate `len` bytes anywhere below the config heap, backed; page-rounded.
    pub(super) fn alloc(&mut self, len: u32) -> Option<HeapObj> {
        let off = self.heap.alloc(len).ok()?;
        self.back(HeapObj { off, len })
    }

    /// Gives an object's heap range and backing pages back. The firmware must hold it no
    /// longer (its cleanup answered) and its MIPS mapping must be gone, flushed.
    pub(super) fn free(&mut self, obj: HeapObj) {
        let first = (obj.off / PAGE) as usize;
        for hp in first..first + obj.len.div_ceil(PAGE) as usize {
            let Some(&index) = self.map.get(hp) else { break };
            if index != UNBACKED {
                let index = usize::from(index);
                self.free_page(Page { index, phys: self.phys[index] });
                self.map[hp] = UNBACKED;
            }
        }
        self.heap.free(obj.off, obj.len);
    }

    /// Every mapped heap page with its backing page's physical address (the MIPS page table).
    pub(super) fn heap_pages(&self) -> impl Iterator<Item = (u32, u64)> + '_ {
        self.map
            .iter()
            .enumerate()
            .filter(|(_, &bp)| bp != UNBACKED)
            .map(|(hp, &bp)| (hp as u32, self.phys[usize::from(bp)]))
    }

    fn backing_of(&self, heap_off: u32) -> Option<(usize, usize)> {
        let bp = *self.map.get((heap_off / PAGE) as usize)?;
        (bp != UNBACKED).then_some((usize::from(bp), (heap_off % PAGE) as usize))
    }

    pub(super) fn phys_at(&self, obj: &HeapObj, at: u32) -> Option<u64> {
        let (bp, in_page) = self.backing_of(obj.off + at)?;
        Some(self.phys[bp] + in_page as u64)
    }

    fn clean(&self, bytes: &[u8]) {
        if self.block != 0 {
            nexus_abi::cache_clean(bytes, self.block);
        }
    }

    fn flush(&self, bytes: &[u8]) {
        if self.block != 0 {
            nexus_abi::cache_flush(bytes, self.block);
        }
    }

    /// Write `bytes` at `at` into `obj` (may cross heap pages), cleaned to memory.
    pub(super) fn write(&mut self, obj: &HeapObj, at: u32, bytes: &[u8]) -> bool {
        if at as usize + bytes.len() > obj.len as usize {
            return false;
        }
        let mut done = 0usize;
        while done < bytes.len() {
            let Some((bp, in_page)) = self.backing_of(obj.off + at + done as u32) else {
                return false;
            };
            let take = (PAGE_BYTES - in_page).min(bytes.len() - done);
            let block = self.block;
            let start = bp * PAGE_BYTES + in_page;
            let dst = &mut self.vmo.bytes_mut()[start..start + take];
            dst.copy_from_slice(&bytes[done..done + take]);
            if block != 0 {
                nexus_abi::cache_clean(dst, block);
            }
            done += take;
        }
        true
    }

    pub(super) fn write_u32(&mut self, obj: &HeapObj, at: u32, v: u32) -> bool {
        self.write(obj, at, &v.to_le_bytes())
    }

    /// The word at `at` in `obj`, flushed from the cache first (the device may have written it).
    pub(super) fn read_u32(&self, obj: &HeapObj, at: u32) -> u32 {
        let Some((bp, in_page)) = self.backing_of(obj.off + at) else { return u32::MAX };
        let start = bp * PAGE_BYTES + in_page;
        let src = &self.vmo.bytes()[start..start + 4];
        self.flush(src);
        u32::from_le_bytes([src[0], src[1], src[2], src[3]])
    }

    /// `len` bytes at `at` in `obj`, flushed first.
    pub(super) fn read(&self, obj: &HeapObj, at: u32, out: &mut [u8]) -> bool {
        let mut done = 0usize;
        while done < out.len() {
            let Some((bp, in_page)) = self.backing_of(obj.off + at + done as u32) else {
                return false;
            };
            let take = (PAGE_BYTES - in_page).min(out.len() - done);
            let start = bp * PAGE_BYTES + in_page;
            let src = &self.vmo.bytes()[start..start + take];
            self.flush(src);
            out[done..done + take].copy_from_slice(src);
            done += take;
        }
        true
    }

    /// A raw backing page's bytes (a page table, the fault page), to fill; cleaned by
    /// [`Self::clean_page`].
    pub(super) fn page_bytes_mut(&mut self, page: &Page) -> &mut [u8] {
        let start = page.index * PAGE_BYTES;
        &mut self.vmo.bytes_mut()[start..start + PAGE_BYTES]
    }

    pub(super) fn clean_page(&self, page: &Page) {
        let start = page.index * PAGE_BYTES;
        self.clean(&self.vmo.bytes()[start..start + PAGE_BYTES]);
    }

    /// A word of a raw page, written and cleaned (a page-table entry).
    pub(super) fn page_write_u32(&mut self, page: &Page, index: usize, v: u32) {
        let start = page.index * PAGE_BYTES + index * 4;
        let block = self.block;
        let dst = &mut self.vmo.bytes_mut()[start..start + 4];
        dst.copy_from_slice(&v.to_le_bytes());
        if block != 0 {
            nexus_abi::cache_clean(dst, block);
        }
    }

    pub(super) fn page_write_u64(&mut self, page: &Page, index: usize, v: u64) {
        let start = page.index * PAGE_BYTES + index * 8;
        let block = self.block;
        let dst = &mut self.vmo.bytes_mut()[start..start + 8];
        dst.copy_from_slice(&v.to_le_bytes());
        if block != 0 {
            nexus_abi::cache_clean(dst, block);
        }
    }
}
