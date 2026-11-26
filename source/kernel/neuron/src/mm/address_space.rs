// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Sv39 address-space management with ASID allocation. Every space sits in a slot at a
//! fixed address behind its own lock (TASK-0330 P3, RFC-0102 rank 3): `get` hands out the space
//! held, and a path that works on one space waits for no other.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: QEMU selftests + boot markers (`KSELFTEST: tables grow ok`, `address space
//!   held by hart ok`, the exec and vm ladders); the lock order on the rank-check lanes
//! PUBLIC API: AddressSpaceManager (create/get/activate/as_map), Space, AsHandle
//! DEPENDS_ON: mm::page_table, arch::riscv (satp), hal::platform
//! INVARIANTS: Map kernel segments; stable SATP value encoding; best-effort W^X for user maps;
//!   a space is never held together with a second one, nor asked for again while it is held
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

use alloc::{boxed::Box, collections::BTreeSet};
use core::num::NonZeroU32;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::sync::block_table::BlockTable;
use crate::sync::lock_rank::Class;
use crate::sync::ranked::{RankedGuard, RankedLock};
use crate::types::{Asid, Pid};

use super::asid::{AsidAllocator, MAX_ASIDS};
use super::kernel_layout::map_kernel_segments;
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use super::page_table::PAGE_SIZE;
use super::page_table::{MapError, PageFlags, PageTable};

static ADDRESS_SPACES_CREATED: AtomicUsize = AtomicUsize::new(0);
static ADDRESS_SPACES_DESTROYED: AtomicUsize = AtomicUsize::new(0);
static ADDRESS_SPACES_LIVE: AtomicUsize = AtomicUsize::new(0);
static ADDRESS_SPACES_PEAK: AtomicUsize = AtomicUsize::new(0);
static ADDRESS_SPACE_PT_PAGES_PEAK: AtomicUsize = AtomicUsize::new(0);

/// Address-space lifecycle and page-table pressure counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AddressSpaceStats {
    /// Address spaces created since boot.
    pub created: usize,
    /// Address spaces destroyed since boot.
    pub destroyed: usize,
    /// Address spaces currently live.
    pub live: usize,
    /// Peak live address spaces.
    pub live_peak: usize,
    /// Largest page-table page count observed for one address space.
    pub per_space_pt_pages_peak: usize,
}

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
#[must_use = "address-space errors must be handled explicitly"]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressSpaceError {
    /// Provided handle was not recognised.
    InvalidHandle,
    /// No more ASIDs are available.
    AsidExhausted,
    /// Address space still has outstanding task references.
    InUse,
    /// A hart's `satp` still names this space's page table (`mm::hart_spaces`).
    HeldByHart,
    /// Address-space destruction is not implemented yet.
    Unsupported,
    /// Underlying map operation failed.
    Mapping(MapError),
    /// Arguments supplied by the caller were invalid.
    InvalidArgs,
    /// User memory a syscall named is not mapped with the access's right (ADR-0077) — `EFAULT`.
    BadUserMemory,
    /// A lazy page of user memory had no free frame to commit (ADR-0077) — `ENOMEM`.
    OutOfFrames,
}

impl From<MapError> for AddressSpaceError {
    fn from(value: MapError) -> Self {
        Self::Mapping(value)
    }
}

/// Tracks the state of a single Sv39 address space.
pub struct AddressSpace {
    page_table: PageTable,
    asid: Asid,
    owners: BTreeSet<Pid>,
    /// RFC-0085: the region table — in the page table's `!Send`/`!Sync`-under-BKL domain.
    va: crate::va_space::VaSpace,
    /// The memory objects those regions name, each counted at its object (RFC-0102).
    maps: crate::cap::SpaceMaps,
    /// ADR-0079: user mode in this space may run `cbo.clean`/`cbo.flush` — granted at the
    /// first such instruction of a process holding a non-coherent device's capability, kept
    /// for the space's life; every activation sets the hart's `senvcfg` from it.
    cache_maintenance: bool,
    /// Its raw handle, from the moment the manager placed it (0 before): what a shootdown for
    /// this space names (`smp::tlb::shootdown_of`).
    raw: u32,
}

// SAFETY: an address space is reached only through its slot's lock (`AddressSpaceManager::get`,
// `each`) or by the path that built it and has not placed it yet; the page-table pages it points
// to are frames it owns, reached through the direct map from any hart.
unsafe impl Send for AddressSpace {}
static_assertions::assert_not_impl_any!(AddressSpace: Sync);

impl AddressSpace {
    fn new(asid: Asid) -> Result<Self, MapError> {
        let mut page_table = PageTable::new()?;
        map_kernel_segments(&mut page_table)?;
        Ok(Self {
            page_table,
            asid,
            owners: BTreeSet::new(),
            va: crate::va_space::VaSpace::new(
                super::USER_VM_WINDOW_BASE,
                super::USER_VM_WINDOW_LEN,
            ),
            maps: crate::cap::SpaceMaps::new(),
            cache_maintenance: false,
            raw: 0,
        })
    }

    /// Its raw handle (0 for a space the manager has not placed).
    pub fn raw(&self) -> u32 {
        self.raw
    }

    /// Whether user mode in this space may maintain cache lines (ADR-0079).
    pub fn cache_maintenance(&self) -> bool {
        self.cache_maintenance
    }

    /// Grant this space user-mode cache maintenance for its life (ADR-0079; the caller
    /// checked the right — a non-coherent device's capability).
    pub fn grant_cache_maintenance(&mut self) {
        self.cache_maintenance = true;
        // Every owner's dispatch switches to the space with the right from now on.
        for pid in &self.owners {
            if let Some(record) = crate::sched::record::get(*pid) {
                record.grant_cache();
            }
        }
    }

    /// This space as a dispatch switches to it, under its handle `handle`.
    pub fn view(&self, handle: AsHandle) -> crate::sched::record::SpaceView {
        crate::sched::record::SpaceView {
            raw: handle.to_raw(),
            satp: self.satp_value(),
            cache: self.cache_maintenance,
        }
    }

    /// Returns the hardware ASID backing this address space.
    pub fn asid(&self) -> Asid {
        self.asid
    }

    /// The region table (RFC-0085) — allocation bookkeeping; the page table
    /// stays the translation truth.
    pub fn va_space(&self) -> &crate::va_space::VaSpace {
        &self.va
    }

    /// The region table and the objects it names, written together — by `space_regions` only.
    pub(super) fn regions_mut(
        &mut self,
    ) -> (&mut crate::va_space::VaSpace, &mut crate::cap::SpaceMaps) {
        (&mut self.va, &mut self.maps)
    }

    /// Borrows the page table for read-only inspection.
    pub fn page_table(&self) -> &PageTable {
        &self.page_table
    }

    /// Returns the SATP value describing this address space.
    pub fn satp_value(&self) -> usize {
        const MODE_SV39: usize = 8;
        let mode = MODE_SV39 << 60;
        let asid = (self.asid.as_raw() as usize) << 44;
        let ppn = self.page_table.root_ppn();
        mode | asid | ppn
    }

    /// Returns page-table pages owned by this address space.
    #[must_use]
    pub fn page_table_pages(&self) -> usize {
        self.page_table.allocated_pages()
    }

    pub(super) fn page_table_mut(&mut self) -> &mut PageTable {
        &mut self.page_table
    }

    fn attach(&mut self, pid: Pid) {
        self.owners.insert(pid);
    }

    fn detach(&mut self, pid: Pid) {
        self.owners.remove(&pid);
    }

    fn refcount(&self) -> usize {
        self.owners.len()
    }
}

/// One slot of the table: a space, or free.
type Slot = RankedLock<Option<Box<AddressSpace>>>;

/// The user address spaces, each at a fixed address behind its own lock (TASK-0330 P3,
/// RFC-0102 rank 3): a path that works on one space waits for no other. A space is never held
/// together with a second one. The registry lock (rank 1) hands out the ASIDs and decides which
/// slot a new space takes; it is never taken while a space is held.
pub struct AddressSpaceManager {
    slots: BlockTable<Slot>,
    asids: RankedLock<AsidAllocator>,
    harts: &'static crate::hart_spaces::HartSpaces,
}

/// An address space, held: its regions and its page table are the holder's until this is
/// dropped. Short holds, interrupts off; nothing of rank 1 or 2 is taken under it, and the
/// manager is not asked for the same space again while it lives.
pub struct Space<'a>(RankedGuard<'a, Option<Box<AddressSpace>>>);

impl core::ops::Deref for Space<'_> {
    type Target = AddressSpace;
    fn deref(&self) -> &AddressSpace {
        match self.0.as_deref() {
            Some(space) => space,
            // `AddressSpaceManager::get` hands out a guard of a full slot only, and nothing
            // empties a slot but `destroy`, which holds this lock.
            None => unreachable!("held address-space slot is empty"),
        }
    }
}

impl core::ops::DerefMut for Space<'_> {
    fn deref_mut(&mut self) -> &mut AddressSpace {
        match self.0.as_deref_mut() {
            Some(space) => space,
            None => unreachable!("held address-space slot is empty"),
        }
    }
}

impl AddressSpaceManager {
    /// Creates an empty manager.
    pub fn new() -> Self {
        let blocks = MAX_ASIDS.div_ceil(crate::sync::block_table::BLOCK);
        let spaces = blocks * crate::sync::block_table::BLOCK;
        let harts =
            crate::hart_spaces::HartSpaces::new(crate::topology::hart_count(), spaces).install();
        Self {
            slots: BlockTable::with_blocks(blocks),
            asids: RankedLock::new(Class::Registry, AsidAllocator::new()),
            harts,
        }
    }

    /// A hart whose `satp` names `handle`'s page table, if any.
    pub fn hart_on(&self, handle: AsHandle) -> Option<usize> {
        self.harts.hart_on(handle.to_raw())
    }

    /// This hart, if it runs on `handle` and no task owns the space any more, switches to
    /// `kernel` — the kernel's own space. A space with an owner is left alone: the caller may be
    /// that owner (a process reaping one of its threads runs on the space it shares with it).
    pub fn leave(&self, handle: AsHandle, kernel: AsHandle) {
        let cpu = crate::smp::cpu_current_id().as_index();
        let unowned = self.get(handle).is_ok_and(|space| space.refcount() == 0);
        if unowned && handle != kernel && self.harts.is_on(cpu, handle.to_raw()) {
            let _ = self.activate(kernel);
        }
    }

    /// Every live address space in turn, each held for its call (accounting, TASK-0286 P5).
    pub fn each(&self, mut f: impl FnMut(&AddressSpace)) {
        for slot in self.slots.iter() {
            if let Some(space) = slot.lock().as_deref() {
                f(space);
            }
        }
    }

    /// Allocates a fresh address space and returns its handle. The space is built outside
    /// every lock; the registry lock is held to name its ASID and again to place it.
    pub fn create(&self) -> Result<AsHandle, AddressSpaceError> {
        let asid = self.asids.lock().allocate().ok_or(AddressSpaceError::AsidExhausted)?;
        let built = AddressSpace::new(asid).map_err(AddressSpaceError::from).and_then(|space| {
            crate::kheap::admits(core::mem::size_of::<AddressSpace>())
                .then(|| Box::new(space))
                .ok_or(AddressSpaceError::OutOfFrames)
        });
        let space = match built {
            Ok(space) => space,
            Err(err) => {
                self.asids.lock().free(asid);
                return Err(err);
            }
        };
        let pt_pages = space.page_table_pages();
        // Under the registry lock a free slot stays free: only this path fills one.
        let mut registry = self.asids.lock();
        let free = self.slots.iter().position(|slot| slot.lock().is_none()).or_else(|| {
            self.slots.grow(crate::kheap::admits, || RankedLock::new(Class::Space, None)).ok()
        });
        let Some((index, slot)) = free.and_then(|at| Some((at, self.slots.get(at)?))) else {
            registry.free(asid);
            return Err(AddressSpaceError::OutOfFrames);
        };
        let handle = AsHandle::from_index(index);
        let mut space = space;
        space.raw = handle.to_raw();
        self.harts.fresh(space.raw);
        *slot.lock() = Some(space);
        drop(registry);
        record_address_space_create(pt_pages);
        Ok(handle)
    }

    /// Returns address-space lifecycle counters.
    #[must_use]
    pub fn stats(&self) -> AddressSpaceStats {
        AddressSpaceStats {
            created: ADDRESS_SPACES_CREATED.load(Ordering::Relaxed),
            destroyed: ADDRESS_SPACES_DESTROYED.load(Ordering::Relaxed),
            live: ADDRESS_SPACES_LIVE.load(Ordering::Relaxed),
            live_peak: ADDRESS_SPACES_PEAK.load(Ordering::Relaxed),
            per_space_pt_pages_peak: ADDRESS_SPACE_PT_PAGES_PEAK.load(Ordering::Relaxed),
        }
    }

    /// The address space `handle` names, held until the result is dropped.
    pub fn get(&self, handle: AsHandle) -> Result<Space<'_>, AddressSpaceError> {
        let held = self.slots.get(handle.index()).ok_or(AddressSpaceError::InvalidHandle)?.lock();
        if held.is_none() {
            return Err(AddressSpaceError::InvalidHandle);
        }
        Ok(Space(held))
    }

    /// Switches the currently running hardware context to `handle`.
    #[track_caller]
    pub fn activate(&self, handle: AsHandle) -> Result<(), AddressSpaceError> {
        let space = self.get(handle)?;
        crate::hart_spaces::enter_space(space.view(handle));
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
        &self,
        handle: AsHandle,
        va: usize,
        set: PageFlags,
    ) -> Result<(), AddressSpaceError> {
        let mut space = self.get(handle)?;
        space.page_table_mut().set_leaf_flags(va, set).map_err(AddressSpaceError::from)?;
        super::vm_ops::fence_asid(space.asid());
        Ok(())
    }

    /// Names `handle` the kernel's own space (boot, once): where a hart with no task runs.
    pub fn name_kernel(&self, handle: AsHandle) -> Result<(), AddressSpaceError> {
        let view = self.get(handle)?.view(handle);
        self.harts.name_kernel(view.raw, view.satp, view.cache);
        Ok(())
    }

    /// Records that `pid` references the provided address space — and gives its scheduling
    /// record the space as its dispatch switches to it (TASK-0363 P2 S3).
    pub fn attach(&self, handle: AsHandle, pid: Pid) -> Result<(), AddressSpaceError> {
        let mut space = self.get(handle)?;
        space.attach(pid);
        let view = space.view(handle);
        if let Some(record) = crate::sched::record::get(pid) {
            record.set_space(view);
        }
        Ok(())
    }

    /// Drops the reference held by `pid` for `handle`.
    pub fn detach(&self, handle: AsHandle, pid: Pid) -> Result<(), AddressSpaceError> {
        let mut space = self.get(handle)?;
        space.detach(pid);
        if let Some(record) = crate::sched::record::get(pid) {
            record.clear_space(handle.to_raw());
        }
        Ok(())
    }

    /// Destroys an address space once no task references remain and no hart runs on it.
    /// The space leaves its slot under the slot's lock; its page table is freed and its ASID
    /// given back after, outside every lock.
    pub fn destroy(&self, handle: AsHandle) -> Result<(), AddressSpaceError> {
        let slot = self.slots.get(handle.index()).ok_or(AddressSpaceError::InvalidHandle)?;
        let gone = {
            let mut held = slot.lock();
            match held.as_deref() {
                None => return Err(AddressSpaceError::InvalidHandle),
                Some(space) if space.refcount() != 0 => return Err(AddressSpaceError::InUse),
                Some(_) if self.harts.hart_on(handle.to_raw()).is_some() => {
                    return Err(AddressSpaceError::HeldByHart)
                }
                Some(_) => held.take(),
            }
        };
        if let Some(space) = gone {
            let asid = space.asid();
            drop(space);
            self.asids.lock().free(asid);
            record_address_space_destroy();
        }
        Ok(())
    }

    /// Maps a single page within the address space referenced by `handle`.
    pub fn map_page(
        &self,
        handle: AsHandle,
        va: usize,
        pa: usize,
        flags: PageFlags,
    ) -> Result<(), AddressSpaceError> {
        Self::map_in(&mut *self.get(handle)?, va, pa, flags)
    }

    /// [`Self::map_page`] on a space already held.
    fn map_in(
        space: &mut AddressSpace,
        va: usize,
        pa: usize,
        flags: PageFlags,
    ) -> Result<(), AddressSpaceError> {
        let res = space.page_table_mut().map(va, pa, flags).map_err(AddressSpaceError::from);
        // Ensure UART pages are marked GLOBAL to remain visible across ASIDs
        if res.is_ok()
            && crate::hal::platform::uart_window().is_some_and(|(b, l)| (b..b + l).contains(&va))
        {
            let _ = space
                .page_table_mut()
                .set_leaf_flags(va, PageFlags::GLOBAL)
                .map_err(AddressSpaceError::from);
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

    /// Tracked fixed-VA map (RFC-0085): refuses the kernel-managed window,
    /// maps, then records the region as `Fixed` (coalescing, counted
    /// overflow). Every legacy fixed-VA path — exec segments, stack,
    /// meta pages, `sys_map`, `sys_mmio_map` — flows through here so the
    /// region table describes the whole address space; the raw
    /// [`Self::map_page`] stays for kernel-internal identity maps only.
    pub fn map_page_tracked(
        &self,
        handle: AsHandle,
        va: usize,
        pa: usize,
        flags: PageFlags,
    ) -> Result<(), AddressSpaceError> {
        let mut space = self.get(handle)?;
        {
            if space.va_space().admits_fixed(va, crate::va_space::PAGE_LEN).is_err() {
                // Either inside the managed window or colliding with a
                // tracked region — both are the caller choosing an address
                // it does not own. EEXIST-class via Overlap keeps ADR-0054's
                // "who was here first" identity.
                log_error!(
                    target: "pt",
                    "VA-FIXED-REFUSED va=0x{:x} (managed window or tracked region)",
                    va
                );
                return Err(AddressSpaceError::Mapping(MapError::Overlap));
            }
        }
        Self::map_in(&mut space, va, pa, flags)?;
        space.va.record_fixed(va, crate::va_space::PAGE_LEN, pa, flags.bits());
        Ok(())
    }

    /// Unmap one page-table leaf (4 KiB or 2 MiB) at `va`, returning the
    /// unmapped size. Region bookkeeping and TLB maintenance are the
    /// CALLER's (the vm_unmap syscall shell) — this is the table edit only.
    pub fn unmap_leaf(&self, handle: AsHandle, va: usize) -> Result<usize, AddressSpaceError> {
        self.get(handle)?.page_table_mut().unmap_leaf(va).map_err(AddressSpaceError::from)
    }
}

fn record_address_space_create(page_table_pages: usize) {
    ADDRESS_SPACES_CREATED.fetch_add(1, Ordering::Relaxed);
    let live = ADDRESS_SPACES_LIVE.fetch_add(1, Ordering::Relaxed) + 1;
    update_peak(&ADDRESS_SPACES_PEAK, live);
    update_peak(&ADDRESS_SPACE_PT_PAGES_PEAK, page_table_pages);
}

fn record_address_space_destroy() {
    ADDRESS_SPACES_DESTROYED.fetch_add(1, Ordering::Relaxed);
    ADDRESS_SPACES_LIVE.fetch_sub(1, Ordering::Relaxed);
}

fn update_peak(counter: &AtomicUsize, value: usize) {
    let mut current = counter.load(Ordering::Relaxed);
    while value > current {
        match counter.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(super) const fn kernel_stack_guard_bytes() -> usize {
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
pub(super) const fn align_down(addr: usize) -> usize {
    addr & !(PAGE_SIZE - 1)
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(super) fn align_up(addr: usize) -> usize {
    let rem = addr % PAGE_SIZE;
    if rem == 0 {
        addr
    } else {
        addr.checked_add(PAGE_SIZE - rem).unwrap_or(usize::MAX & !(PAGE_SIZE - 1))
    }
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(crate) fn fence_i() {
    unsafe {
        core::arch::asm!("fence.i", options(nostack));
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
#[allow(dead_code)]
pub(super) fn fence_i() {}
