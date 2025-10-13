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
        let mut page_table = PageTable::new();
        map_kernel_segments(&mut page_table)?;
        Ok(Self { page_table, asid, owners: BTreeSet::new() })
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
        Self { spaces: Vec::new(), asids: AsidAllocator::new() }
    }

    /// Allocates a fresh address space and returns its handle.
    pub fn create(&mut self) -> Result<AsHandle, AddressSpaceError> {
        let asid = self.asids.allocate().ok_or(AddressSpaceError::AsidExhausted)?;
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
    pub fn activate(&self, handle: AsHandle) -> Result<(), AddressSpaceError> {
        let space = self.get(handle)?;
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        unsafe {
            // SAFETY: caller ensures exclusive access when switching SATP.
            riscv::register::satp::write(space.satp_value());
            core::arch::asm!("sfence.vma x0, x0", options(nostack));
        }
        Ok(())
    }

    /// Records that `pid` references the provided address space.
    pub fn attach(&mut self, handle: AsHandle, pid: u32) -> Result<(), AddressSpaceError> {
        let space = self.get_mut(handle)?;
        space.attach(pid);
        Ok(())
    }

    /// Drops the reference held by `pid` for `handle`.
    pub fn detach(&mut self, handle: AsHandle, pid: u32) -> Result<(), AddressSpaceError> {
        let space = self.get_mut(handle)?;
        space.detach(pid);
        Ok(())
    }

    /// Stub implementation of address-space destruction.
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
    pub fn map_page(
        &mut self,
        handle: AsHandle,
        va: usize,
        pa: usize,
        flags: PageFlags,
    ) -> Result<(), AddressSpaceError> {
        let space = self.get_mut(handle)?;
        space.page_table_mut().map(va, pa, flags).map_err(AddressSpaceError::from)
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
    let text_end = align_up(unsafe { &__text_end as *const u8 as usize });
    map_identity_range(
        table,
        text_start,
        text_end,
        PageFlags::VALID | PageFlags::READ | PageFlags::EXECUTE | PageFlags::GLOBAL,
    )?;

    let data_start = text_end;
    let data_end = align_up(unsafe { &__bss_end as *const u8 as usize });
    if data_end > data_start {
        map_identity_range(
            table,
            data_start,
            data_end,
            PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
        )?;
    }

    let stack_start = align_down(unsafe { &__stack_bottom as *const u8 as usize });
    let stack_end = align_up(unsafe { &__stack_top as *const u8 as usize });
    if stack_end > stack_start {
        map_identity_range(
            table,
            stack_start,
            stack_end,
            PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
        )?;
    }

    const UART_BASE: usize = 0x1000_0000;
    const UART_LEN: usize = 0x1000;
    let uart_start = align_down(UART_BASE);
    let uart_end = align_up(UART_BASE + UART_LEN);
    map_identity_range(
        table,
        uart_start,
        uart_end,
        PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::GLOBAL,
    )?;

    Ok(())
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn map_kernel_segments(_table: &mut PageTable) -> Result<(), MapError> {
    Ok(())
}

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

const fn align_down(addr: usize) -> usize {
    addr & !(PAGE_SIZE - 1)
}

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

