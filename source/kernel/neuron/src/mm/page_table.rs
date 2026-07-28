// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Sv39 page-table implementation with lazy allocation of intermediate levels
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: QEMU selftests + boot markers
//! PUBLIC API: PageTable (new/map/unmap/lookup/verify), PageFlags, MapError, PAGE_SIZE
//! DEPENDS_ON: bitflags, core alloc (optional static pool behind features)
//! INVARIANTS: Enforce W^X (`PermissionDenied`), canonical Sv39 ranges, 4096-byte alignment
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::{boxed::Box, vec, vec::Vec};
use core::marker::PhantomData;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use bitflags::bitflags;

/// Size of a single page in bytes.
pub const PAGE_SIZE: usize = 4096;
/// Number of entries per Sv39 page-table page.
pub(super) const PT_ENTRIES: usize = 512;
/// Size of an Sv39 level-1 leaf mapping.
pub const HUGE_PAGE_SIZE_2M: usize = 2 * 1024 * 1024;

static HEAP_PT_PAGES_LIVE: AtomicUsize = AtomicUsize::new(0);
static HEAP_PT_PAGES_TOTAL: AtomicUsize = AtomicUsize::new(0);
static HEAP_PT_PAGES_PEAK: AtomicUsize = AtomicUsize::new(0);

/// Heap-backed page-table allocation counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageTableAllocationStats {
    /// Heap-backed page-table pages currently live.
    pub heap_live: usize,
    /// Heap-backed page-table pages ever allocated.
    pub heap_total: usize,
    /// Peak heap-backed page-table pages live at one time.
    pub heap_peak: usize,
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    /// Flags stored in Sv39 page-table entries.
    pub struct PageFlags: usize {
        const VALID = 1 << 0;
        const READ = 1 << 1;
        const WRITE = 1 << 2;
        const EXECUTE = 1 << 3;
        const USER = 1 << 4;
        const GLOBAL = 1 << 5;
        const ACCESSED = 1 << 6;
        const DIRTY = 1 << 7;
    }
}

/// Error returned when manipulating page tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapError {
    /// Virtual or physical address was not page aligned.
    Unaligned,
    /// Mapping extends beyond the canonical Sv39 range.
    OutOfRange,
    /// Mapping violates the W^X policy.
    PermissionDenied,
    /// Mapping collides with an existing entry.
    Overlap,
    /// Flags do not describe a valid leaf entry.
    InvalidFlags,
    /// Unmap: no leaf is mapped at this address (RFC-0085). Reaching this
    /// from `vm_unmap` means the region table and the page table diverged —
    /// a kernel invariant violation, logged loudly at the call site.
    NotMapped,
}

#[repr(align(4096))]
pub(super) struct PageTablePage {
    pub(super) entries: [usize; PT_ENTRIES],
}

impl PageTablePage {
    const fn new() -> Self {
        Self { entries: [0; PT_ENTRIES] }
    }
}

// Optional static root page for early bring-up to avoid allocator/intrinsics.
// The PageTablePage type already carries 4096-byte alignment via #[repr(align(4096))].
#[cfg(feature = "pt_static_root")]
static mut PT_STATIC_ROOT: PageTablePage = PageTablePage::new();

// Optional static pool of page-table pages for early bring-up to avoid heap usage.
#[cfg(feature = "bringup_identity")]
static mut PT_STATIC_POOL: [PageTablePage; 64] = [const { PageTablePage::new() }; 64];
// SMP A2c: atomic bump cursor — the pool hand-out must stay race-free once
// secondary harts allocate page tables (each index is claimed exactly once).
#[cfg(feature = "bringup_identity")]
static PT_STATIC_POOL_NEXT: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "bringup_identity")]
const PT_STATIC_POOL_CAP: usize = 64;

/// Three-level Sv39 page table allocating intermediate levels on demand.
pub struct PageTable {
    pub(super) root: NonNull<PageTablePage>,
    owned: Vec<NonNull<PageTablePage>>,
    // Pre-SMP contract: page-table mutation remains single-context until SMP VM ownership split.
    _not_send_sync: PhantomData<*mut ()>,
}
static_assertions::assert_not_impl_any!(PageTable: Send, Sync);

impl PageTable {
    /// Creates an empty Sv39 page table with a fresh root page.
    pub fn new() -> Self {
        #[cfg(feature = "pt_static_root")]
        unsafe {
            // SAFETY: The static page is uniquely used as the root for this instance in
            // early bring-up; higher-level code must ensure single use or add a manager.
            let ptr: *mut PageTablePage = core::ptr::addr_of_mut!(PT_STATIC_ROOT);
            let root = NonNull::new_unchecked(ptr);
            return Self { root, owned: vec![], _not_send_sync: PhantomData };
        }
        #[cfg(not(feature = "pt_static_root"))]
        {
            let root = Self::alloc_page();
            Self { root, owned: vec![root], _not_send_sync: PhantomData }
        }
    }

    /// Returns the physical page number of the root page suitable for SATP.
    pub fn root_ppn(&self) -> usize {
        self.root.as_ptr() as usize / PAGE_SIZE
    }

    /// Returns page-table pages owned by this address space.
    #[must_use]
    pub fn allocated_pages(&self) -> usize {
        self.owned.len()
    }

    /// Returns global heap-backed page-table allocation counters.
    #[must_use]
    pub fn allocation_stats() -> PageTableAllocationStats {
        PageTableAllocationStats {
            heap_live: HEAP_PT_PAGES_LIVE.load(Ordering::Relaxed),
            heap_total: HEAP_PT_PAGES_TOTAL.load(Ordering::Relaxed),
            heap_peak: HEAP_PT_PAGES_PEAK.load(Ordering::Relaxed),
        }
    }

    /// Looks up the entry mapped at `va` if it exists.
    pub fn lookup(&self, va: usize) -> Option<usize> {
        if va % PAGE_SIZE != 0 || !is_canonical_sv39(va) {
            return None;
        }
        let indices = vpn_indices(va);
        let mut table = self.root;
        for (level, index) in indices.iter().enumerate() {
            let entry = unsafe { (*table.as_ptr()).entries[*index] };
            if entry & PageFlags::VALID.bits() == 0 {
                return None;
            }
            let is_leaf = entry & LEAF_PERMS.bits() != 0;
            if is_leaf {
                return Some(entry);
            }
            if level == indices.len() - 1 {
                return None;
            }
            let next = ((entry >> 10) << 12) as *mut PageTablePage;
            table = NonNull::new(next)?;
        }
        None
    }

    /// Translates an arbitrary virtual address to a physical address if mapped.
    pub fn translate(&self, va: usize) -> Option<usize> {
        if !is_canonical_sv39(va) {
            return None;
        }
        let indices = vpn_indices(va);
        let mut table = self.root;
        for (level, index) in indices.iter().enumerate() {
            let entry = unsafe { (*table.as_ptr()).entries[*index] };
            if entry & PageFlags::VALID.bits() == 0 {
                return None;
            }
            let is_leaf = entry & LEAF_PERMS.bits() != 0;
            if is_leaf {
                let ppn = entry >> 10;
                let page_shift = 12 + (2 - level) * 9;
                let page_size = 1usize << page_shift;
                let phys_base = (ppn << 12) & !(page_size - 1);
                let offset = va & (page_size - 1);
                return Some(phys_base | offset);
            }
            let next = ((entry >> 10) << 12) as *mut PageTablePage;
            table = NonNull::new(next)?;
        }
        None
    }

    /// Returns the leaf flags for the mapping at `va`.
    pub fn leaf_flags(&self, va: usize) -> Result<PageFlags, MapError> {
        if va % PAGE_SIZE != 0 || !is_canonical_sv39(va) {
            return Err(MapError::OutOfRange);
        }
        let indices = vpn_indices(va);
        let mut table = self.root;
        for (level, index) in indices.iter().enumerate() {
            let entry = unsafe { (*table.as_ptr()).entries[*index] };
            if entry & PageFlags::VALID.bits() == 0 {
                return Err(MapError::OutOfRange);
            }
            let is_leaf = entry & LEAF_PERMS.bits() != 0;
            if is_leaf {
                let flags = PageFlags::from_bits_truncate(entry & 0x3FF);
                return Ok(flags);
            }
            if level == indices.len() - 1 {
                return Err(MapError::OutOfRange);
            }
            let next = ((entry >> 10) << 12) as *mut PageTablePage;
            table = NonNull::new(next).ok_or(MapError::OutOfRange)?;
        }
        Err(MapError::OutOfRange)
    }

    /// Names the occupant when a map is refused as [`MapError::Overlap`]
    /// (TASK-0309). Error path only — never on the map hot path.
    ///
    /// `Overlap` has two distinct causes (an existing 4 KiB leaf, or a
    /// superpage covering the VA from an intermediate level) and by the time
    /// the refusal reaches a service it has collapsed into a bare EINVAL. A
    /// service can say *my* address and *my* offset; only the kernel can say
    /// what is ALREADY THERE — so it must be the one to say it.
    fn trace_overlap(kind: &str, level: usize, va: usize, pa: usize, entry: usize) {
        log_error!(
            target: "pt",
            "PT-OVERLAP kind={} level={} va=0x{:x} want_pa=0x{:x} occupant_pa=0x{:x} occupant_flags=0x{:x}",
            kind,
            level,
            va,
            pa,
            (entry >> 10) << 12,
            entry & 0x3ff
        );
    }

    /// Installs a 4 KiB mapping from `va` to `pa` using `flags`.
    pub fn map(&mut self, va: usize, pa: usize, flags: PageFlags) -> Result<(), MapError> {
        if va % PAGE_SIZE != 0 || pa % PAGE_SIZE != 0 {
            return Err(MapError::Unaligned);
        }
        if !is_canonical_sv39(va) {
            return Err(MapError::OutOfRange);
        }
        if flags.intersection(LEAF_PERMS).is_empty() || !flags.contains(PageFlags::VALID) {
            return Err(MapError::InvalidFlags);
        }
        if flags.contains(PageFlags::WRITE) && flags.contains(PageFlags::EXECUTE) {
            return Err(MapError::PermissionDenied);
        }

        let indices = vpn_indices(va);
        let mut effective_flags = flags | PageFlags::ACCESSED;
        if flags.contains(PageFlags::WRITE) {
            effective_flags |= PageFlags::DIRTY;
        }
        let mut table = self.root;
        for (level, index) in indices.iter().enumerate() {
            let entry = unsafe { &mut (*table.as_ptr()).entries[*index] };
            if level == indices.len() - 1 {
                if *entry & PageFlags::VALID.bits() != 0 {
                    // TASK-0309: `Overlap` has two causes and userspace cannot
                    // tell them apart — every kernel map failure reaches an app
                    // as EINVAL. Name the occupant here so a boot log can, at
                    // least, say WHO is already at this address.
                    Self::trace_overlap("leaf", level, va, pa, *entry);
                    return Err(MapError::Overlap);
                }
                let ppn = pa / PAGE_SIZE;
                *entry = (ppn << 10) | effective_flags.bits();
                return Ok(());
            }

            if *entry & PageFlags::VALID.bits() != 0 {
                if *entry & LEAF_PERMS.bits() != 0 {
                    // A SUPERPAGE covers this VA: the walk hit leaf permissions
                    // on an intermediate level.
                    Self::trace_overlap("superpage", level, va, pa, *entry);
                    return Err(MapError::Overlap);
                }
                let next = ((*(entry) >> 10) << 12) as *mut PageTablePage;
                table = NonNull::new(next).ok_or(MapError::OutOfRange)?;
                continue;
            }

            let next = Self::alloc_page();
            self.owned.push(next);
            let ppn = next.as_ptr() as usize / PAGE_SIZE;
            *entry = (ppn << 10) | PageFlags::VALID.bits();
            table = next;
        }
        Ok(())
    }

    /// Clears the leaf covering `va` and returns the size it mapped
    /// (4 KiB or 2 MiB). The FIRST unmap primitive this kernel has had —
    /// the module header advertised one for months while the code could
    /// only destroy whole address spaces (RFC-0085).
    ///
    /// Alignment: `va` must be page-aligned; a 2 MiB leaf additionally
    /// requires `va` to be superpage-aligned (v1 unmaps exactly what was
    /// mapped — no splitting). Intermediate table pages are NOT reclaimed
    /// (bounded: reused by remaps, freed at address-space destroy).
    ///
    /// The caller owns TLB maintenance: this only edits the table.
    pub(crate) fn unmap_leaf(&mut self, va: usize) -> Result<usize, MapError> {
        if va % PAGE_SIZE != 0 {
            return Err(MapError::Unaligned);
        }
        if !is_canonical_sv39(va) {
            return Err(MapError::OutOfRange);
        }
        let indices = vpn_indices(va);
        let mut table = self.root;
        for (level, index) in indices.iter().enumerate() {
            let entry = unsafe { &mut (*table.as_ptr()).entries[*index] };
            if *entry & PageFlags::VALID.bits() == 0 {
                return Err(MapError::NotMapped);
            }
            let is_leaf = *entry & LEAF_PERMS.bits() != 0;
            if is_leaf {
                // Level 1 = 2 MiB superpage, level 2 = 4 KiB page. A 1 GiB
                // leaf (level 0) is never installed for user mappings.
                let size = match level {
                    1 => HUGE_PAGE_SIZE_2M,
                    2 => PAGE_SIZE,
                    _ => return Err(MapError::NotMapped),
                };
                if size == HUGE_PAGE_SIZE_2M && va % HUGE_PAGE_SIZE_2M != 0 {
                    // Asked to unmap mid-superpage: v1 has no splitting.
                    return Err(MapError::Unaligned);
                }
                *entry = 0;
                return Ok(size);
            }
            let next = ((*entry >> 10) << 12) as *mut PageTablePage;
            table = NonNull::new(next).ok_or(MapError::OutOfRange)?;
        }
        Err(MapError::NotMapped)
    }

    /// Installs a 2 MiB Sv39 leaf mapping.
    pub(crate) fn map_2m(
        &mut self,
        va: usize,
        pa: usize,
        flags: PageFlags,
    ) -> Result<(), MapError> {
        if va % HUGE_PAGE_SIZE_2M != 0 || pa % HUGE_PAGE_SIZE_2M != 0 {
            return Err(MapError::Unaligned);
        }
        if !is_canonical_sv39(va) {
            return Err(MapError::OutOfRange);
        }
        if flags.intersection(LEAF_PERMS).is_empty() || !flags.contains(PageFlags::VALID) {
            return Err(MapError::InvalidFlags);
        }
        if flags.contains(PageFlags::WRITE) && flags.contains(PageFlags::EXECUTE) {
            return Err(MapError::PermissionDenied);
        }

        let indices = vpn_indices(va);
        let mut effective_flags = flags | PageFlags::ACCESSED;
        if flags.contains(PageFlags::WRITE) {
            effective_flags |= PageFlags::DIRTY;
        }
        let mut table = self.root;
        for (level, index) in indices.iter().enumerate() {
            let entry = unsafe { &mut (*table.as_ptr()).entries[*index] };
            if level == 1 {
                if *entry & PageFlags::VALID.bits() != 0 {
                    return Err(MapError::Overlap);
                }
                let ppn = pa / PAGE_SIZE;
                *entry = (ppn << 10) | effective_flags.bits();
                return Ok(());
            }

            if *entry & PageFlags::VALID.bits() != 0 {
                if *entry & LEAF_PERMS.bits() != 0 {
                    return Err(MapError::Overlap);
                }
                let next = ((*entry >> 10) << 12) as *mut PageTablePage;
                table = NonNull::new(next).ok_or(MapError::OutOfRange)?;
                continue;
            }

            let next = Self::alloc_page();
            self.owned.push(next);
            let ppn = next.as_ptr() as usize / PAGE_SIZE;
            *entry = (ppn << 10) | PageFlags::VALID.bits();
            table = next;
        }
        Err(MapError::OutOfRange)
    }

    /// Updates the leaf flags at `va` by OR-ing with `set`.
    /// Returns `OutOfRange` if no mapping exists at `va`.
    pub fn set_leaf_flags(&mut self, va: usize, set: PageFlags) -> Result<(), MapError> {
        if va % PAGE_SIZE != 0 || !is_canonical_sv39(va) {
            return Err(MapError::OutOfRange);
        }
        let indices = vpn_indices(va);
        let mut table = self.root;
        for (level, index) in indices.iter().enumerate() {
            let entry = unsafe { &mut (*table.as_ptr()).entries[*index] };
            if *entry & PageFlags::VALID.bits() == 0 {
                return Err(MapError::OutOfRange);
            }
            let is_leaf = *entry & LEAF_PERMS.bits() != 0;
            if level == indices.len() - 1 {
                if !is_leaf {
                    return Err(MapError::OutOfRange);
                }
                let current_flags = *entry & 0x3FF; // low 10 bits are flags
                let new_flags = current_flags | set.bits();
                // Enforce W^X: do not permit WRITE+EXECUTE concurrently
                let w = (new_flags & PageFlags::WRITE.bits()) != 0;
                let x = (new_flags & PageFlags::EXECUTE.bits()) != 0;
                if w && x {
                    return Err(MapError::PermissionDenied);
                }
                let ppn_part = *entry & !0x3FF; // keep PPN bits intact
                *entry = ppn_part | new_flags;
                return Ok(());
            }
            if is_leaf {
                return Err(MapError::OutOfRange);
            }
            let next = ((*entry >> 10) << 12) as *mut PageTablePage;
            table = NonNull::new(next).ok_or(MapError::OutOfRange)?;
        }
        Err(MapError::OutOfRange)
    }

    /// Updates the leaf flags at `va` by clearing `clear` and setting `set` bits.
    pub fn update_leaf_flags(
        &mut self,
        va: usize,
        clear: PageFlags,
        set: PageFlags,
    ) -> Result<(), MapError> {
        if va % PAGE_SIZE != 0 || !is_canonical_sv39(va) {
            return Err(MapError::OutOfRange);
        }
        let indices = vpn_indices(va);
        let mut table = self.root;
        for (level, index) in indices.iter().enumerate() {
            let entry = unsafe { &mut (*table.as_ptr()).entries[*index] };
            if *entry & PageFlags::VALID.bits() == 0 {
                return Err(MapError::OutOfRange);
            }
            let is_leaf = *entry & LEAF_PERMS.bits() != 0;
            if level == indices.len() - 1 {
                if !is_leaf {
                    return Err(MapError::OutOfRange);
                }
                let current_flags = *entry & 0x3FF;
                let new_flags = (current_flags & !clear.bits()) | set.bits();
                // Enforce W^X: do not permit WRITE+EXECUTE concurrently
                let w = (new_flags & PageFlags::WRITE.bits()) != 0;
                let x = (new_flags & PageFlags::EXECUTE.bits()) != 0;
                if w && x {
                    return Err(MapError::PermissionDenied);
                }
                let ppn_part = *entry & !0x3FF;
                *entry = ppn_part | new_flags;
                return Ok(());
            }
            if is_leaf {
                return Err(MapError::OutOfRange);
            }
            let next = ((*entry >> 10) << 12) as *mut PageTablePage;
            table = NonNull::new(next).ok_or(MapError::OutOfRange)?;
        }
        Err(MapError::OutOfRange)
    }

    /// UNSAFE: Updates leaf flags at `va` by OR-ing with `set` bits without
    /// enforcing W^X. Intended for early kernel bring-up when kernel stack and
    /// text may overlap in the same page and we must temporarily allow RWX.
    pub unsafe fn set_leaf_flags_unchecked(
        &mut self,
        va: usize,
        set: PageFlags,
    ) -> Result<(), MapError> {
        if va % PAGE_SIZE != 0 || !is_canonical_sv39(va) {
            return Err(MapError::OutOfRange);
        }
        let indices = vpn_indices(va);
        let mut table = self.root;
        for (level, index) in indices.iter().enumerate() {
            let entry = unsafe { &mut (*table.as_ptr()).entries[*index] };
            if *entry & PageFlags::VALID.bits() == 0 {
                return Err(MapError::OutOfRange);
            }
            let is_leaf = *entry & LEAF_PERMS.bits() != 0;
            if level == indices.len() - 1 {
                if !is_leaf {
                    return Err(MapError::OutOfRange);
                }
                let current_flags = *entry & 0x3FF;
                let new_flags = current_flags | set.bits();
                let ppn_part = *entry & !0x3FF;
                *entry = ppn_part | new_flags;
                return Ok(());
            }
            if is_leaf {
                return Err(MapError::OutOfRange);
            }
            let next = ((*entry >> 10) << 12) as *mut PageTablePage;
            table = NonNull::new(next).ok_or(MapError::OutOfRange)?;
        }
        Err(MapError::OutOfRange)
    }

    fn alloc_page() -> NonNull<PageTablePage> {
        #[cfg(feature = "bringup_identity")]
        {
            // Atomic claim via bounded CAS (lr/sc): each index is handed out
            // exactly once even with concurrent allocators on other harts,
            // and the cursor never grows past the cap (heap fallback after).
            // NOTE: deliberately lr/sc-based (same primitive class as the
            // spin locks used throughout) rather than an AMO fetch_add — an
            // amoadd here reproducibly wedged the boot (see SMP track A2c).
            let mut idx = PT_STATIC_POOL_NEXT.load(Ordering::Acquire);
            loop {
                if idx >= PT_STATIC_POOL_CAP {
                    break;
                }
                match PT_STATIC_POOL_NEXT.compare_exchange_weak(
                    idx,
                    idx + 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => break,
                    Err(observed) => idx = observed,
                }
            }
            if idx < PT_STATIC_POOL_CAP {
                // SAFETY: `idx` was claimed exclusively above; the pool is a
                // static with 'static lifetime.
                unsafe {
                    let base: *mut [PageTablePage; PT_STATIC_POOL_CAP] =
                        core::ptr::addr_of_mut!(PT_STATIC_POOL);
                    let first: *mut PageTablePage = base as *mut PageTablePage;
                    let page_ptr: *mut PageTablePage = first.add(idx);
                    return NonNull::new_unchecked(page_ptr);
                }
            }
        }
        let boxed = Box::new(PageTablePage::new());
        record_heap_page_alloc();
        unsafe { NonNull::new_unchecked(Box::into_raw(boxed)) }
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        #[cfg(not(feature = "bringup_identity"))]
        {
            for page in self.owned.drain(..) {
                // SAFETY: every pointer originates from `alloc_page` and is unique.
                unsafe { drop(Box::from_raw(page.as_ptr())) };
                record_heap_page_free();
            }
        }
        #[cfg(feature = "bringup_identity")]
        {
            let static_base =
                core::ptr::addr_of_mut!(PT_STATIC_POOL) as *mut PageTablePage as usize;
            let static_end =
                static_base + PT_STATIC_POOL_CAP * core::mem::size_of::<PageTablePage>();
            for page in self.owned.drain(..) {
                let ptr = page.as_ptr() as usize;
                if ptr >= static_base && ptr < static_end {
                    continue;
                }
                // SAFETY: non-static pointers originate from Box allocations in `alloc_page`.
                unsafe { drop(Box::from_raw(page.as_ptr())) };
                record_heap_page_free();
            }
        }
    }
}

fn record_heap_page_alloc() {
    let live = HEAP_PT_PAGES_LIVE.fetch_add(1, Ordering::Relaxed) + 1;
    HEAP_PT_PAGES_TOTAL.fetch_add(1, Ordering::Relaxed);
    let mut peak = HEAP_PT_PAGES_PEAK.load(Ordering::Relaxed);
    while live > peak {
        match HEAP_PT_PAGES_PEAK.compare_exchange_weak(
            peak,
            live,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => peak = observed,
        }
    }
}

fn record_heap_page_free() {
    HEAP_PT_PAGES_LIVE.fetch_sub(1, Ordering::Relaxed);
}

pub(super) const LEAF_PERMS: PageFlags =
    PageFlags::READ.union(PageFlags::WRITE).union(PageFlags::EXECUTE);

pub(super) fn vpn_indices(va: usize) -> [usize; 3] {
    let vpn0 = (va >> 12) & 0x1ff;
    let vpn1 = (va >> 21) & 0x1ff;
    let vpn2 = (va >> 30) & 0x1ff;
    // Traverse from the top level (VPN2) down to VPN0 to match Sv39 walk order
    [vpn2, vpn1, vpn0]
}

pub const fn is_canonical_sv39(va: usize) -> bool {
    let sign = (va >> 38) & 1;
    let upper = va >> 39;
    if sign == 0 {
        upper == 0
    } else {
        upper == usize::MAX >> 39
    }
}
