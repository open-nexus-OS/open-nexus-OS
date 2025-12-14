// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Sv39 address-space management with ASID allocation
//! OWNERS: @kernel-mm-team
//! PUBLIC API: AddressSpaceManager (create/get/activate/as_map), AsHandle
//! DEPENDS_ON: mm::page_table, arch::riscv (satp), hal::virt
//! INVARIANTS: Map kernel segments; stable SATP value encoding; best-effort W^X for user maps
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::{collections::BTreeSet, vec::Vec};
use core::num::NonZeroU32;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use super::page_table::PAGE_SIZE;
use super::page_table::{MapError, PageFlags, PageTable};
use super::USER_VMO_ARENA_LEN;

/// Maximum ASIDs made available by the allocator.
const MAX_ASIDS: usize = 256;
const WORD_BITS: usize = core::mem::size_of::<u64>() * 8;
const BITMAP_WORDS: usize = (MAX_ASIDS + WORD_BITS - 1) / WORD_BITS;

/// Handle referencing a tracked address space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AsHandle(NonZeroU32);

impl AsHandle {
    /// Creates a handle from the backing table index.
    fn from_index(index: usize) -> Self {
        // SAFETY: index is offset by one, ensuring the raw value is never zero.
        unsafe { Self(NonZeroU32::new_unchecked(index as u32 + 1)) }
    }

    /// Returns the table index backing this handle.
    fn index(self) -> usize {
        self.0.get() as usize - 1
    }

    /// Constructs a handle from a raw value provided by userspace.
    pub fn from_raw(raw: u32) -> Option<Self> {
        NonZeroU32::new(raw).map(Self)
    }

    /// Returns the raw representation of the handle.
    pub fn to_raw(self) -> u32 {
        self.0.get()
    }
}

/// Errors reported while managing address spaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressSpaceError {
    /// Provided handle was not recognised.
    InvalidHandle,
    /// No more ASIDs are available.
    AsidExhausted,
    /// Address space still has outstanding task references.
    InUse,
    /// Address-space destruction is not implemented yet.
    Unsupported,
    /// Underlying map operation failed.
    Mapping(MapError),
    /// Arguments supplied by the caller were invalid.
    InvalidArgs,
}

impl From<MapError> for AddressSpaceError {
    fn from(value: MapError) -> Self {
        Self::Mapping(value)
    }
}

/// Tracks the state of a single Sv39 address space.
pub struct AddressSpace {
    page_table: PageTable,
    asid: u16,
    owners: BTreeSet<u32>,
}

impl AddressSpace {
    fn new(asid: u16) -> Result<Self, MapError> {
        let mut page_table = PageTable::new();
        map_kernel_segments(&mut page_table)?;
        Ok(Self {
            page_table,
            asid,
            owners: BTreeSet::new(),
        })
    }

    /// Returns the hardware ASID backing this address space.
    pub fn asid(&self) -> u16 {
        self.asid
    }

    /// Borrows the page table for read-only inspection.
    pub fn page_table(&self) -> &PageTable {
        &self.page_table
    }

    /// Returns the SATP value describing this address space.
    pub fn satp_value(&self) -> usize {
        const MODE_SV39: usize = 8;
        let mode = MODE_SV39 << 60;
        let asid = (self.asid as usize) << 44;
        let ppn = self.page_table.root_ppn();
        mode | asid | ppn
    }

    fn page_table_mut(&mut self) -> &mut PageTable {
        &mut self.page_table
    }

    fn attach(&mut self, pid: u32) {
        self.owners.insert(pid);
    }

    fn detach(&mut self, pid: u32) {
        self.owners.remove(&pid);
    }

    fn refcount(&self) -> usize {
        self.owners.len()
    }
}

/// Manages the collection of user address spaces and allocates ASIDs.
pub struct AddressSpaceManager {
    spaces: Vec<Option<AddressSpace>>,
    asids: AsidAllocator,
}

impl AddressSpaceManager {
    /// Creates an empty manager.
    pub fn new() -> Self {
        let mgr = Self {
            spaces: Vec::new(),
            asids: AsidAllocator::new(),
        };
        mgr
    }

    /// Allocates a fresh address space and returns its handle.
    pub fn create(&mut self) -> Result<AsHandle, AddressSpaceError> {
        let asid = self
            .asids
            .allocate()
            .ok_or(AddressSpaceError::AsidExhausted)?;
        let space = match AddressSpace::new(asid) {
            Ok(space) => space,
            Err(err) => {
                self.asids.free(asid);
                return Err(AddressSpaceError::from(err));
            }
        };
        for (index, slot) in self.spaces.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(space);
                return Ok(AsHandle::from_index(index));
            }
        }
        self.spaces.push(Some(space));
        Ok(AsHandle::from_index(self.spaces.len() - 1))
    }

    /// Returns a shared reference to the address space identified by `handle`.
    pub fn get(&self, handle: AsHandle) -> Result<&AddressSpace, AddressSpaceError> {
        self.spaces
            .get(handle.index())
            .and_then(|slot| slot.as_ref())
            .ok_or(AddressSpaceError::InvalidHandle)
    }

    /// Returns a mutable reference to the address space identified by `handle`.
    pub fn get_mut(&mut self, handle: AsHandle) -> Result<&mut AddressSpace, AddressSpaceError> {
        self.spaces
            .get_mut(handle.index())
            .and_then(|slot| slot.as_mut())
            .ok_or(AddressSpaceError::InvalidHandle)
    }

    /// Switches the currently running hardware context to `handle`.
    #[must_use]
    pub fn activate(&self, handle: AsHandle) -> Result<(), AddressSpaceError> {
        let space = self.get(handle)?;
        #[cfg(feature = "selftest_no_satp")]
        let _ = &space; // silence unused when SATP switching is disabled
        #[cfg(all(
            target_arch = "riscv64",
            target_os = "none",
            not(feature = "selftest_no_satp")
        ))]
        {
            // CRITICAL: Separate function to ensure all logs/temporaries drop BEFORE AS switch
            do_preflight_checks_and_switch(space.satp_value());
        }
        #[cfg(all(
            target_arch = "riscv64",
            target_os = "none",
            feature = "selftest_no_satp"
        ))]
        {
            // Skip SATP switch for bring-up; emit post-switch marker directly
            fence_i();
            log_info!(target: "as", "AS: post-satp OK");
        }
        #[cfg(debug_assertions)]
        {
            // Best-effort verify the active page table in debug builds.
            if let Err(_e) = space.page_table().verify() {
                // CRITICAL: After satp switch, use safe UART logging only
                use core::fmt::Write;
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("PT-VERIFY: violation after activate\n");
            }
        }
        Ok(())
    }

    /// Sets additional leaf flags for a mapping at `va` in the address space referenced by `handle`.
    pub fn set_leaf_flags(
        &mut self,
        handle: AsHandle,
        va: usize,
        set: PageFlags,
    ) -> Result<(), AddressSpaceError> {
        let space = self.get_mut(handle)?;
        space
            .page_table_mut()
            .set_leaf_flags(va, set)
            .map_err(AddressSpaceError::from)
    }

    #[cfg(all(
        target_arch = "riscv64",
        target_os = "none",
        feature = "bringup_identity",
        not(feature = "selftest_no_satp")
    ))]
    #[allow(dead_code)]
    pub fn activate_via_trampoline(&self, handle: AsHandle) -> Result<(), AddressSpaceError> {
        self.activate(handle)
    }

    /// Records that `pid` references the provided address space.
    #[must_use]
    pub fn attach(&mut self, handle: AsHandle, pid: u32) -> Result<(), AddressSpaceError> {
        let space = self.get_mut(handle)?;
        space.attach(pid);
        Ok(())
    }

    /// Drops the reference held by `pid` for `handle`.
    #[must_use]
    pub fn detach(&mut self, handle: AsHandle, pid: u32) -> Result<(), AddressSpaceError> {
        let space = self.get_mut(handle)?;
        space.detach(pid);
        Ok(())
    }

    /// Stub implementation of address-space destruction.
    #[must_use]
    pub fn destroy(&mut self, handle: AsHandle) -> Result<(), AddressSpaceError> {
        let slot = self
            .spaces
            .get_mut(handle.index())
            .ok_or(AddressSpaceError::InvalidHandle)?;
        match slot.as_ref() {
            None => Err(AddressSpaceError::InvalidHandle),
            Some(space) if space.refcount() != 0 => Err(AddressSpaceError::InUse),
            Some(_) => Err(AddressSpaceError::Unsupported),
        }
    }

    /// Maps a single page within the address space referenced by `handle`.
    #[must_use]
    pub fn map_page(
        &mut self,
        handle: AsHandle,
        va: usize,
        pa: usize,
        flags: PageFlags,
    ) -> Result<(), AddressSpaceError> {
        let space = self.get_mut(handle)?;
        let res = space
            .page_table_mut()
            .map(va, pa, flags)
            .map_err(AddressSpaceError::from);
        // Ensure kernel text/UART pages are marked GLOBAL to remain visible across ASIDs
        if res.is_ok() {
            if va >= 0x8000_0000 && va < 0x8100_0000 {
                let _ = space
                    .page_table_mut()
                    .set_leaf_flags(va, PageFlags::GLOBAL)
                    .map_err(AddressSpaceError::from);
            }
            if va >= 0x1000_0000 && va < 0x1000_1000 {
                let _ = space
                    .page_table_mut()
                    .set_leaf_flags(va, PageFlags::GLOBAL)
                    .map_err(AddressSpaceError::from);
            }
        }
        #[cfg(debug_assertions)]
        {
            if let Err(ref _e) = res {
                log_error!(target: "pt", "PT-VERIFY: map error");
            } else if let Err(_e) = space.page_table().verify() {
                log_error!(target: "pt", "PT-VERIFY: violation after map");
            }
        }
        res
    }
}

struct AsidAllocator {
    bitmap: [u64; BITMAP_WORDS],
    next: usize,
}

impl AsidAllocator {
    const fn new() -> Self {
        let mut bitmap = [0u64; BITMAP_WORDS];
        // Reserve ASID 0 for the kernel/global mappings.
        bitmap[0] |= 1;
        Self { bitmap, next: 1 }
    }

    fn allocate(&mut self) -> Option<u16> {
        for _ in 0..MAX_ASIDS {
            let index = self.next % MAX_ASIDS;
            let word = index / WORD_BITS;
            let bit = index % WORD_BITS;
            if self.bitmap[word] & (1 << bit) == 0 {
                self.bitmap[word] |= 1 << bit;
                self.next = (index + 1) % MAX_ASIDS;
                return Some(index as u16);
            }
            self.next = (index + 1) % MAX_ASIDS;
        }
        None
    }

    fn free(&mut self, asid: u16) {
        let index = asid as usize;
        if index < MAX_ASIDS {
            let word = index / WORD_BITS;
            let bit = index % WORD_BITS;
            self.bitmap[word] &= !(1 << bit);
        }
    }
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn map_kernel_segments(table: &mut PageTable) -> Result<(), MapError> {
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
        log_info!(target: "mm", "AS-MAP: HEAP OK in DATA range: heap={:#x}..{:#x} data_end={:#x}",
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
    let mapped_start = stack_start
        .checked_add(guard_bytes)
        .ok_or(MapError::OutOfRange)?;
    // DATA/BSS identity range may already cover the low portion of the stack; avoid remapping.
    let map_from = core::cmp::max(mapped_start, data_end);
    log_info!(
        target: "mm",
        "AS-MAP: KSTACK check: start={:#x} end={:#x} data_end={:#x} guard={} map_from={:#x}",
        stack_start,
        stack_end,
        data_end,
        guard_bytes,
        map_from
    );
    if map_from < stack_end {
        log_info!(target: "mm", "AS-MAP: mapping KSTACK tail {:#x}..{:#x}", map_from, stack_end);
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
        log_info!(target: "mm", "AS-MAP: KSTACK tail mapped ok");
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
    // This matches the temporary page pool used by the user_loader.
    let pool_base = 0x8060_0000usize;
    let pool_end = pool_base + 2 * 1024 * 1024;
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

    // Identity-map the per-service VMO arena after the kernel pools/stacks so
    // userspace VMOs never alias kernel text/data.
    let vmo_base = core::cmp::max(align_up(stack_end), align_up(pool_end));
    let vmo_end = vmo_base
        .checked_add(USER_VMO_ARENA_LEN)
        .ok_or(MapError::OutOfRange)?;
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

    log_info!(target: "mm", "map kernel segments ok");
    Ok(())
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn map_kernel_segments(_table: &mut PageTable) -> Result<(), MapError> {
    Ok(())
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn map_identity_range(
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
        table.map(addr, addr, flags)?;
        addr = addr.checked_add(PAGE_SIZE).ok_or(MapError::OutOfRange)?;
    }
    Ok(())
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
const fn kernel_stack_guard_bytes() -> usize {
    STACK_GUARD_PAGES * PAGE_SIZE
}

#[cfg(all(
    target_arch = "riscv64",
    target_os = "none",
    any(debug_assertions, feature = "debug_stack_guards")
))]
const STACK_GUARD_PAGES: usize = 1;
#[cfg(all(
    target_arch = "riscv64",
    target_os = "none",
    not(any(debug_assertions, feature = "debug_stack_guards"))
))]
const STACK_GUARD_PAGES: usize = 0;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
const fn align_down(addr: usize) -> usize {
    addr & !(PAGE_SIZE - 1)
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn align_up(addr: usize) -> usize {
    let rem = addr % PAGE_SIZE;
    if rem == 0 {
        addr
    } else {
        addr.checked_add(PAGE_SIZE - rem)
            .unwrap_or_else(|| usize::MAX & !(PAGE_SIZE - 1))
    }
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn fence_i() {
    unsafe {
        core::arch::asm!("fence.i", options(nostack));
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
#[allow(dead_code)]
fn fence_i() {}

#[cfg(all(
    target_arch = "riscv64",
    target_os = "none",
    not(feature = "selftest_no_satp")
))]
#[inline(never)]
fn do_preflight_checks_and_switch(satp_val: usize) {
    const LOG_LIMIT: usize = 16;
    static LOG_COUNT: core::sync::atomic::AtomicUsize =
        core::sync::atomic::AtomicUsize::new(0);
    let log_now = LOG_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < LOG_LIMIT;

    // SAFE logging using raw UART - no heap allocation, logs in separate scopes
    if log_now {
        use core::fmt::Write;
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("AS: activating satp=0x");
        crate::trap::uart_write_hex(&mut u, satp_val);
        let _ = u.write_str("\n");
    }

    // Preflight: verify PC is executable
    if log_now {
        use core::fmt::Write;
        let mut u = crate::uart::raw_writer();
        let pc = crate::arch::riscv::read_pc() & !(PAGE_SIZE - 1);
        let _ = u.write_str("AS: pc check va=0x");
        crate::trap::uart_write_hex(&mut u, pc);
        let _ = u.write_str("\n");
    }

    // Preflight: verify SP is accessible
    if log_now {
        use core::fmt::Write;
        let mut u = crate::uart::raw_writer();
        let sp = crate::arch::riscv::read_sp() & !(PAGE_SIZE - 1);
        let _ = u.write_str("AS: sp check va=0x");
        crate::trap::uart_write_hex(&mut u, sp);
        let _ = u.write_str("\n");
    }

    ensure_rx_guard();

    // Call satp switch - any code after this runs in new AS!
    unsafe {
        extern "C" {
            fn satp_switch_island(val: usize);
        }
        satp_switch_island(satp_val);
    }
    // Function returns - back to activate() which is now in new AS
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[allow(dead_code)]
fn ensure_rx_guard() {
    const LOG_LIMIT: usize = 16;
    static LOG_COUNT: core::sync::atomic::AtomicUsize =
        core::sync::atomic::AtomicUsize::new(0);
    let log_now = LOG_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < LOG_LIMIT;

    let pc = crate::arch::riscv::read_pc();
    if log_now {
        use core::fmt::Write;
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("AS: ensure_rx_guard pc=0x");
        crate::trap::uart_write_hex(&mut u, pc);
        let _ = u.write_str("\n");
    }
    let mut zero = true;
    let mut offset = 0usize;
    while offset < 8 {
        let byte = unsafe { core::ptr::read_volatile((pc + offset) as *const u8) };
        if byte != 0 {
            zero = false;
        }
        offset += 1;
    }
    if zero {
        panic!("rx guard: zero fetch");
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
#[allow(dead_code)]
fn ensure_rx_guard() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_unique_asids() {
        let mut manager = AddressSpaceManager::new();
        let mut seen = alloc::collections::BTreeSet::new();
        for _ in 0..(MAX_ASIDS - 1) {
            let handle = manager.create().expect("allocate");
            let asid = manager.get(handle).unwrap().asid();
            assert!(seen.insert(asid));
        }
        assert_eq!(
            manager.create().unwrap_err(),
            AddressSpaceError::AsidExhausted
        );
    }
}
