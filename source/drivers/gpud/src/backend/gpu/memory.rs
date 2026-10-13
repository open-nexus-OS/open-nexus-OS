// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The firmware heap's backing (TASK-0329 G1): ONE page-backed DMA VMO made for the GPU's
//! capability (its pages inside the device's reach; `vmo_runs` names them), handed out page by
//! page — a heap object at a heap offset (fixed: the code and data regions, the three config
//! slots; first-fit: everything else) is backed by one backing page per heap page, and the MIPS
//! page table maps the heap page to the backing page's physical address. The GPU MMU's own
//! table pages and the fault page come from the same pool. The SoC is non-coherent: a word the
//! CPU wrote is cleaned to memory before the device may read it; a word the device wrote is
//! flushed from the cache before the CPU reads it. In this layer one command is in flight at a
//! time, so a shared cache line never carries a CPU write and a firmware write at once (the
//! uncached mapping that lifts this is the memory lane's — recorded in the ledger).

use alloc::vec::Vec;

use nexus_abi::{Cap, DmaVmo};
use nexus_gfx::backend::gpu::mips_boot::{self, HeapMap, PAGE};

/// Bytes of one page.
const PAGE_BYTES: usize = PAGE as usize;

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
    /// Backing pages handed out so far.
    next: usize,
    heap: HeapMap,
    /// Heap page → backing page.
    map: Vec<(u32, usize)>,
}

impl FwMemory {
    /// `pages` backing pages for the device behind `device`; `block` is the device's cache
    /// block (0 for a coherent one).
    pub(super) fn new(device: Cap, pages: usize, block: usize) -> Result<Self, &'static str> {
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
        Ok(Self { vmo, block, phys, next: 0, heap: HeapMap::new(), map: Vec::new() })
    }

    pub(super) const fn block(&self) -> usize {
        self.block
    }

    /// Backing pages still free.
    pub(super) fn free_pages(&self) -> usize {
        self.phys.len() - self.next
    }

    /// One zeroed backing page (the kernel zeroed the object at create).
    pub(super) fn alloc_page(&mut self) -> Option<Page> {
        let index = self.next;
        let phys = *self.phys.get(index)?;
        self.next += 1;
        Some(Page { index, phys })
    }

    fn back(&mut self, obj: HeapObj) -> Option<HeapObj> {
        let first = obj.off / PAGE;
        let count = obj.len.div_ceil(PAGE);
        for i in 0..count {
            let page = self.alloc_page()?;
            self.map.push((first + i, page.index));
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
        self.back(HeapObj { off, len: PAGE })
    }

    /// Allocate `len` bytes anywhere below the config heap, backed; page-rounded.
    pub(super) fn alloc(&mut self, len: u32) -> Option<HeapObj> {
        let off = self.heap.alloc(len).ok()?;
        self.back(HeapObj { off, len })
    }

    /// Every mapped heap page with its backing page's physical address (the MIPS page table).
    pub(super) fn heap_pages(&self) -> impl Iterator<Item = (u32, u64)> + '_ {
        self.map.iter().map(|&(hp, bp)| (hp, self.phys[bp]))
    }

    fn backing_of(&self, heap_off: u32) -> Option<(usize, usize)> {
        let hp = heap_off / PAGE;
        let bp = self.map.iter().find(|(h, _)| *h == hp).map(|(_, b)| *b)?;
        Some((bp, (heap_off % PAGE) as usize))
    }

    /// The physical address of `at` bytes into `obj`.
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
