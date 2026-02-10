// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Sv39 page-table implementation with lazy allocation of intermediate levels
//! OWNERS: @kernel-mm-team
//! PUBLIC API: PageTable (new/map/unmap/lookup/verify), PageFlags, MapError, PAGE_SIZE
//! DEPENDS_ON: bitflags, core alloc (optional static pool behind features)
//! INVARIANTS: Enforce W^X (`PermissionDenied`), canonical Sv39 ranges, 4096-byte alignment
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::{boxed::Box, vec, vec::Vec};
use core::marker::PhantomData;
use core::ptr::NonNull;

use bitflags::bitflags;

/// Size of a single page in bytes.
pub const PAGE_SIZE: usize = 4096;
/// Number of entries per Sv39 page-table page.
const PT_ENTRIES: usize = 512;

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
}

#[repr(align(4096))]
struct PageTablePage {
    entries: [usize; PT_ENTRIES],
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
#[cfg(feature = "bringup_identity")]
static mut PT_STATIC_POOL_NEXT: usize = 0;
#[cfg(feature = "bringup_identity")]
const PT_STATIC_POOL_CAP: usize = 64;

/// Three-level Sv39 page table allocating intermediate levels on demand.
pub struct PageTable {
    root: NonNull<PageTablePage>,
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
            if level == indices.len() - 1 {
                return if is_leaf { Some(entry) } else { None };
            }
            if is_leaf {
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
        unsafe {
            if PT_STATIC_POOL_NEXT < PT_STATIC_POOL_CAP {
                let idx = PT_STATIC_POOL_NEXT;
                PT_STATIC_POOL_NEXT = PT_STATIC_POOL_NEXT + 1;
                // Obtain base pointer to the first element without creating a shared ref
                let base: *mut [PageTablePage; PT_STATIC_POOL_CAP] =
                    core::ptr::addr_of_mut!(PT_STATIC_POOL);
                let first: *mut PageTablePage = base as *mut PageTablePage;
                let page_ptr: *mut PageTablePage = first.add(idx);
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = write!(u, "PT: pool idx={}\n", idx);
                }
                return NonNull::new_unchecked(page_ptr);
            }
        }
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "PT: heap alloc\n");
        }
        let boxed = Box::new(PageTablePage::new());
        unsafe { NonNull::new_unchecked(Box::into_raw(boxed)) }
    }

    /// Debug-only invariant checker for the Sv39 page table.
    /// Verifies that:
    /// - Non-leaf entries do not carry leaf permission bits
    /// - Leaf entries are VALID and carry at least one of R/W/X
    /// - W^X is enforced (never both WRITE and EXECUTE)
    /// This is a best-effort walk that assumes the internal pointers
    /// are well-formed; only compiled when debug assertions or the
    /// `debug_pt_verify` feature is enabled.
    #[cfg(debug_assertions)]
    pub fn verify(&self) -> Result<(), &'static str> {
        unsafe fn walk(page: *const PageTablePage) -> Result<(), &'static str> {
            for i in 0..PT_ENTRIES {
                let entry = unsafe { (*page).entries[i] };
                if entry == 0 {
                    continue;
                }
                let valid = entry & PageFlags::VALID.bits() != 0;
                if !valid {
                    return Err("pt: nonzero but !VALID");
                }
                let is_leaf = entry & LEAF_PERMS.bits() != 0;
                if is_leaf {
                    let has_perm = entry & LEAF_PERMS.bits() != 0;
                    if !has_perm {
                        return Err("pt: leaf without perms");
                    }
                    let w = entry & PageFlags::WRITE.bits() != 0;
                    let x = entry & PageFlags::EXECUTE.bits() != 0;
                    if w && x {
                        return Err("pt: W^X violated");
                    }
                } else {
                    // Non-leaf must not carry any leaf perms
                    if entry & LEAF_PERMS.bits() != 0 {
                        return Err("pt: non-leaf has leaf perms");
                    }
                    let next = ((entry >> 10) << 12) as *const PageTablePage;
                    // Recurse into the next level
                    unsafe { walk(next)? };
                }
            }
            Ok(())
        }

        unsafe { walk(self.root.as_ptr()) }
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        #[cfg(not(feature = "bringup_identity"))]
        {
            for page in self.owned.drain(..) {
                // SAFETY: every pointer originates from `alloc_page` and is unique.
                unsafe { drop(Box::from_raw(page.as_ptr())) };
            }
        }
        #[cfg(feature = "bringup_identity")]
        {
            // Skip freeing static pool pages during bring-up.
            self.owned.clear();
        }
    }
}

const LEAF_PERMS: PageFlags = PageFlags::READ.union(PageFlags::WRITE).union(PageFlags::EXECUTE);

fn vpn_indices(va: usize) -> [usize; 3] {
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
