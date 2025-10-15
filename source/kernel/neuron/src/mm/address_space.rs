// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Sv39 address-space management with ASID allocation.

extern crate alloc;

use alloc::{collections::BTreeSet, vec::Vec};
use core::num::NonZeroU32;

use super::page_table::{MapError, PageFlags, PageTable, PAGE_SIZE};

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
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "AS: new enter asid=0x{:x}\n", asid);
        }
        let mut page_table = PageTable::new();
        map_kernel_segments(&mut page_table)?;
        let s = Self { page_table, asid, owners: BTreeSet::new() };
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "AS: new exit\n");
        }
        Ok(s)
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
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "AS-MGR: new enter\n");
        }
        let mgr = Self { spaces: Vec::new(), asids: AsidAllocator::new() };
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "AS-MGR: new exit\n");
        }
        mgr
    }

    /// Allocates a fresh address space and returns its handle.
    pub fn create(&mut self) -> Result<AsHandle, AddressSpaceError> {
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "AS-MGR: create enter\n");
        }
        let asid = self.asids.allocate().ok_or(AddressSpaceError::AsidExhausted)?;
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "AS-MGR: asid=0x{:x}\n", asid);
        }
        let space = match AddressSpace::new(asid) {
            Ok(space) => space,
            Err(err) => {
                self.asids.free(asid);
                return Err(AddressSpaceError::from(err));
            }
        };
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "AS-MGR: after AddressSpace::new\n");
        }
        for (index, slot) in self.spaces.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(space);
                return Ok(AsHandle::from_index(index));
            }
        }
        self.spaces.push(Some(space));
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "AS-MGR: create exit\n");
        }
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
        #[cfg(all(target_arch = "riscv64", target_os = "none", not(feature = "selftest_no_satp")))]
        unsafe {
            {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = write!(u, "AS: before satp write val=0x{:x}\n", space.satp_value());
            }
            // Switch SATP to this address space and flush TLBs.
            riscv::register::satp::write(space.satp_value());
            core::arch::asm!("sfence.vma x0, x0", options(nostack));
            #[cfg(feature = "bringup_identity")]
            {
                // After switching, set SP to a known-good stack inside the identity-mapped stack band
                extern "C" { static __stack_top: u8; }
                let sp_new = &__stack_top as *const u8 as usize - 64;
                core::arch::asm!("mv sp, {ns}", ns = in(reg) sp_new, options(nostack));
            }
        }
        #[cfg(debug_assertions)]
        {
            // Best-effort verify the active page table in debug builds.
            if let Err(_e) = space.page_table().verify() {
                // Keep diagnostics minimal on OS path; emit marker only.
                crate::uart::write_line("PT-VERIFY: violation after activate");
            }
        }
        Ok(())
    }

        #[cfg(all(target_arch = "riscv64", target_os = "none", feature = "bringup_identity", not(feature = "selftest_no_satp")))]
    #[allow(dead_code)]
    pub fn activate_via_trampoline(&self, handle: AsHandle) -> Result<(), AddressSpaceError> {
        let space = self.get(handle)?;
        unsafe {
            extern "C" { fn satp_switch_island(val: usize); }
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "AS: tramp call val=0x{:x}\n", space.satp_value());
            satp_switch_island(space.satp_value());
        }
        Ok(())
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
        let res = space.page_table_mut().map(va, pa, flags).map_err(AddressSpaceError::from);
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
                crate::uart::write_line("PT-VERIFY: map error");
            } else if let Err(_e) = space.page_table().verify() {
                crate::uart::write_line("PT-VERIFY: violation after map");
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
    }

    let text_start = align_down(unsafe { &__text_start as *const u8 as usize });
    let mut text_end = align_up(unsafe { &__text_end as *const u8 as usize });
    if text_end <= text_start {
        // Fallback: if linker symbols are not providing a usable range, map a minimal RX window
        // to ensure we can execute immediately after SATP activation during bring-up.
        let raw_stack_bottom = unsafe { &__stack_bottom as *const u8 as usize };
        let stack_start = align_down(raw_stack_bottom);
        let fallback_end = text_start
            .checked_add(PAGE_SIZE * 64)
            .unwrap_or(usize::MAX & !(PAGE_SIZE - 1));
        text_end = core::cmp::min(fallback_end, stack_start);
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "AS-MAP: text fallback to [0x{:x}..0x{:x})\n", text_start, text_end);
        }
    }
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = write!(u, "AS-MAP: before text 0x{:x}..0x{:x}\n", text_start, text_end);
    }
    map_identity_range(
        table,
        text_start,
        text_end,
        PageFlags::VALID | PageFlags::READ | PageFlags::EXECUTE | PageFlags::GLOBAL,
    )?;
    // (Bring-up safety band removed to avoid overlaps; stack mapping below is sufficient.)
    // Ensure instruction cache coherence after installing text mappings
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        core::arch::asm!("fence.i", options(nostack));
    }
    // Bisect: return early to test whether text mapping path is safe.
    #[cfg(feature = "as_map_bisect_text_only")]
    {
        return Ok(());
    }
    #[cfg(not(feature = "as_map_bisect_text_only"))]
    {
        // Ensure text is RX and data/bss are RW before switching SATP.
        {
            let mut addr = text_start;
            while addr < text_end {
                let _ = table.update_leaf_flags(addr, PageFlags::WRITE, PageFlags::EXECUTE);
                addr = addr.checked_add(PAGE_SIZE).ok_or(MapError::OutOfRange)?;
            }
        }
        #[cfg(debug_assertions)]
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let sample = table.lookup(text_start).unwrap_or(0) & 0xFF;
            let _ = write!(u, "AS-MAP: text [0x{:x}..0x{:x}) flags=0x{:x}\n", text_start, text_end, sample);
        }

        let data_start = text_end;
        let mut data_end = align_up(unsafe { &__bss_end as *const u8 as usize });
        if data_end <= data_start {
            data_end = data_start;
        }
        if data_end > data_start {
            {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = write!(u, "AS-MAP: before data 0x{:x}..0x{:x}\n", data_start, data_end);
            }
            map_identity_range(
                table,
                data_start,
                data_end,
                PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
            )?;
            // Some early pages may be shared between text and data; if an entry already exists
            // with EXECUTE, allow WRITE alongside for bring-up to avoid stalls post-switch.
            {
                let mut addr = data_start;
                while addr < data_end {
                    if let Some(pte) = table.lookup(addr) {
                        let has_x = (pte & PageFlags::EXECUTE.bits()) != 0;
                        if has_x {
                            // UNSAFE: temporary RWX allowed for bring-up stability
                            let _ = unsafe { table.set_leaf_flags_unchecked(addr, PageFlags::WRITE) };
                        }
                    }
                    addr = addr.checked_add(PAGE_SIZE).ok_or(MapError::OutOfRange)?;
                }
            }
            {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = write!(u, "AS-MAP: after data\n");
            }
            #[cfg(debug_assertions)]
            {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let sample = table.lookup(data_start).unwrap_or(0) & 0xFF;
                let _ = write!(u, "AS-MAP: data [0x{:x}..0x{:x}) flags=0x{:x}\n", data_start, data_end, sample);
            }
        }

        let mut stack_start = align_down(unsafe { &__stack_bottom as *const u8 as usize });
        let stack_end = align_up(unsafe { &__stack_top as *const u8 as usize });
        if stack_start < text_end {
            stack_start = text_end;
        }
        if stack_end > stack_start {
            {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = write!(u, "AS-MAP: before stack 0x{:x}..0x{:x}\n", stack_start, stack_end);
            }
            #[cfg(feature = "bringup_identity")]
            {
                // Map stack band as RX then add WRITE unchecked to allow temporary RWX.
                let mut addr = stack_start;
                while addr < stack_end {
                    table.map(
                        addr,
                        addr,
                        PageFlags::VALID | PageFlags::READ | PageFlags::EXECUTE | PageFlags::GLOBAL,
                    )?;
                    let _ = unsafe { table.set_leaf_flags_unchecked(addr, PageFlags::WRITE) };
                    addr = addr.checked_add(PAGE_SIZE).ok_or(MapError::OutOfRange)?;
                }
            }
            #[cfg(not(feature = "bringup_identity"))]
            {
                map_identity_range(
                    table,
                    stack_start,
                    stack_end,
                    PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
                )?;
            }
            {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = write!(u, "AS-MAP: after stack\n");
            }
            #[cfg(debug_assertions)]
            {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let sample = table.lookup(stack_start).unwrap_or(0) & 0xFF;
                let _ = write!(u, "AS-MAP: stack [0x{:x}..0x{:x}) flags=0x{:x}\n", stack_start, stack_end, sample);
            }
        }

        const UART_BASE: usize = 0x1000_0000;
        const UART_LEN: usize = 0x1000;
        let uart_start = align_down(UART_BASE);
        let uart_end = align_up(UART_BASE + UART_LEN);
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "AS-MAP: before uart 0x{:x}..0x{:x}\n", uart_start, uart_end);
        }
        map_identity_range(
            table,
            uart_start,
            uart_end,
            PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
        )?;
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "AS-MAP: after uart\n");
        }
        #[cfg(debug_assertions)]
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let sample = table.lookup(uart_start).unwrap_or(0) & 0xFF;
            let _ = write!(u, "AS-MAP: uart [0x{:x}..0x{:x}) flags=0x{:x}\n", uart_start, uart_end, sample);
        }

        // Bring-up: ensure a conservative identity window for the kernel is present after SATP
        // switch. We merge with any existing mappings to avoid overlaps. Enabled only when
        // feature `bringup_identity` is active.
        #[cfg(feature = "bringup_identity")]
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let identity_start: usize = 0x8020_0000;
            let identity_end: usize = 0x8028_0000; // 512 KiB window around early PCs/return
            let _ = write!(u, "AS-MAP: bringup identity merge 0x{:x}..0x{:x}\n", identity_start, identity_end);
            let mut addr = identity_start;
            while addr < identity_end {
                let mut need_map = true;
                if let Some(_pte) = table.lookup(addr) {
                    // Try to OR-in missing perms (WRITE) unchecked; if it fails, we'll map below
                    if unsafe { table.set_leaf_flags_unchecked(addr, PageFlags::READ | PageFlags::WRITE | PageFlags::EXECUTE | PageFlags::GLOBAL) }.is_ok() {
                        need_map = false;
                    }
                }
                if need_map {
                    // Map as RX first (respects W^X), then add WRITE unchecked to reach RWX
                    let _ = table.map(
                        addr,
                        addr,
                        PageFlags::VALID | PageFlags::READ | PageFlags::EXECUTE | PageFlags::GLOBAL,
                    );
                    let _ = unsafe { table.set_leaf_flags_unchecked(addr, PageFlags::WRITE) };
                }
                addr = addr.checked_add(PAGE_SIZE).ok_or(MapError::OutOfRange)?;
            }
        }

        return Ok(());
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn map_kernel_segments(_table: &mut PageTable) -> Result<(), MapError> {
    Ok(())
}

#[allow(dead_code)]
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

#[allow(dead_code)]
const fn align_down(addr: usize) -> usize {
    addr & !(PAGE_SIZE - 1)
}

#[allow(dead_code)]
fn align_up(addr: usize) -> usize {
    let rem = addr % PAGE_SIZE;
    if rem == 0 {
        addr
    } else {
        addr
            .checked_add(PAGE_SIZE - rem)
            .unwrap_or_else(|| usize::MAX & !(PAGE_SIZE - 1))
    }
}

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
        assert_eq!(manager.create().unwrap_err(), AddressSpaceError::AsidExhausted);
    }
}
