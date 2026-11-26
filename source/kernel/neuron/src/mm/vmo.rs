// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the page-backed VMO (TASK-0286 P3a, RFC-0098 C4). An object is a
//! list of physically contiguous blocks from the frame pool: `Anon` takes the
//! largest blocks that fit (greedy, so a 2 MiB-aligned run stays a superpage
//! when mapped), `Contiguous` takes ONE block — for a DMA master that needs one
//! base (a virtqueue). An object made for a device comes from inside that
//! device's DMA reach (`devices`, TASK-0246 P1) — a lazy one commits each page
//! there (ADR-0078); its physical runs leave the kernel only through
//! `vmo_runs`, in the asking device's bus addresses, and naming a lazy range
//! commits it (`named_runs`: a device cannot fault). `Fixed`
//! wraps frames that never came from the pool (the tree, a kernel data page)
//! and are never returned. Capabilities carry the id (`Vmo { id, len }`,
//! `VmoRo { id, len }`); this table is the ONE owner of the frames behind them
//! — `VmoPool` and the fixed arena are gone.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: block placement is host-tested in `crate::frames`; the object
//!   paths are proven by the VMO selftests and every service image
//! INVARIANTS: an id names at most one live object; frames of a destroyed
//!   object are back in the pool before the id is reusable; a `Fixed` object
//!   never frees; the table lock is a leaf (never held across a memset or a codec call — a
//!   squeezed page's record is copied in and out under it, ADR-0087).

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::frame_pool;
use crate::cap::{Held, HeldCounts, Rights};
use crate::dma_reach::{DmaReach, MAX_DMA_WINDOWS};
use crate::frames::{Block, FrameError, FRAME_SIZE, MAX_ORDER};
use crate::lazy::{LazyPages, PAGE};

#[cfg(debug_assertions)]
type TableLock<T> = crate::sync::dbg_mutex::DbgMutex<T>;
#[cfg(not(debug_assertions))]
type TableLock<T> = spin::Mutex<T>;

/// A live object's id: never 0, the table index plus one.
pub type VmoId = u32;

/// A new object: its id and its first capability (`MAP`), made in one hold of the table
/// (RFC-0102). Whoever made it keeps the capability until the object is named by what it was
/// made for — a slot, a region — and drops it then; an object nobody holds and nobody maps is
/// the lifetime rule's (`reclaim`).
pub type Born = (VmoId, Held);

/// A lazy page gets its frame (ADR-0077): the whole answer, and the part a fault runs unlocked.
mod commit;
pub use commit::{commit, commit_fresh};
/// The address spaces that map an object, counted at it (RFC-0102): the token a space takes.
pub mod mapping;
/// Purgeable objects (RFC-0100 §4): the unlock, the lock, the purge at `warn`.
pub mod purge;
/// Sealed objects (ADR-0080): the seal, the device-named mark that forbids it.
mod sealing;
pub use sealing::{is_sealed, name_for_device, seal, SealError};
/// An object's bytes as physical pieces; a lazy range as the runs a device is told.
mod span;
pub use span::{named_runs, span};
/// CPU writes made visible to a non-coherent device (ADR-0092): `vmo_clean`, cleaned zeroes.
pub mod clean;
/// Squeezed pages (RFC-0100 §5, ADR-0087): a page recorded, a page brought back.
pub mod squeeze;
/// Swapped pages (RFC-0100 §5, ADR-0088): a record exported, committed, brought back.
pub mod swap;

/// How an object is backed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmoKind {
    /// Pool blocks, largest-first; no physical base is promised.
    Anon,
    /// One physically contiguous run (one DMA base): a block of the next power of two whose
    /// tail beyond the object goes straight back to the pool.
    Contiguous,
    /// Frames outside the pool (never freed).
    Fixed,
    /// No frame until a page is first touched (ADR-0077): one zeroed frame per 4 KiB page,
    /// committed by a user fault, the kernel's checked range, a write, or a device's runs.
    Lazy,
}

/// Why a table call was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmoError {
    /// Zero, unaligned, or (contiguous) beyond the largest block.
    BadLength,
    /// The pool could not back it; the frames taken so far went back.
    Exhausted,
    /// No live object has this id.
    NoSuchObject,
    /// A squeezed page's record did not decode to one page (ADR-0087): the access fails, the
    /// record stays.
    Damaged,
    /// The page is swapped (ADR-0088): it comes back through the pager — a fault or a syscall
    /// parks on `token` until it does; any other access fails.
    Swapped { token: u64, slot: u32 },
    /// The kernel heap above its reserve cannot hold the object's bookkeeping — its page words
    /// or its block list (ADR-0091): the caller hears `ENOMEM`, the kernel never panics.
    HeapExhausted,
    /// A capability is still held to the object (RFC-0102): it is not destroyed under one —
    /// a held capability never names an id the table gave to another object.
    Held,
    /// An address space still maps the object (its own count, RFC-0102): freeing under a live
    /// mapping would hand recycled frames to the mapper.
    Mapped,
}

/// The largest object a caller may ask for (ADR-0091): the frames the pool was ever given — no
/// object needs more than the memory that could back it. `None` before the pool exists.
fn max_object_bytes() -> Option<usize> {
    frame_pool::stats().map(|stats| stats.total.saturating_mul(FRAME_SIZE as usize))
}

/// The CPU ranges a lazy object made for a device takes its frames from (ADR-0078): its
/// device's reach, kept with the object because its pages are committed long after the create.
#[derive(Clone, Copy)]
struct Within {
    ranges: [(u64, u64); MAX_DMA_WINDOWS],
    n: usize,
}

impl Within {
    /// `reach` as ranges, or `None` when it reaches every frame.
    fn of(reach: &DmaReach) -> Option<Self> {
        if reach.is_all() {
            return None;
        }
        let mut within = Self { ranges: [(0, 0); MAX_DMA_WINDOWS], n: 0 };
        for range in reach.cpu_ranges().take(MAX_DMA_WINDOWS) {
            within.ranges[within.n] = range;
            within.n += 1;
        }
        Some(within)
    }

    fn ranges(&self) -> &[(u64, u64)] {
        &self.ranges[..self.n]
    }
}

/// One object.
pub struct VmoObject {
    kind: VmoKind,
    len: usize,
    blocks: Vec<Block>,
    fixed: (usize, usize),
    /// A lazy object's frames, one entry per page; `None` for every other kind.
    lazy: Option<LazyPages>,
    /// Where a lazy object made for a device commits its pages; `None`: anywhere.
    within: Option<Within>,
    /// ADR-0080: sealed — no writer for the rest of its life (`vmo_write`, a writable map and
    /// `vmo_runs` are refused); the only kind of object `exec_vmo` maps a process from.
    sealed: bool,
    /// A device was told this object's runs (`vmo_runs`): it may write them at any time, so the
    /// object can never be sealed.
    device_named: bool,
    /// RFC-0100 §4: unlocked — the owner said the content may go; a purge drops the pages, and
    /// the object is untouchable (no map, write, read, runs) until its lock.
    purgeable: bool,
    /// A purge took the content since the unlock; the owner learns it at the lock.
    purged: bool,
    /// ADR-0092: the cache block a CPU write must be cleaned in before the object's device reads
    /// it — 0 when that device snoops the caches, or there is none. Set before any capability.
    clean_block: u16,
    /// The capabilities held to this object, in a table or in a message (RFC-0102): the
    /// header the table gives it with its id (`VmoTable::insert`); `None` only before that.
    held: Option<Arc<HeldCounts>>,
}

impl VmoObject {
    /// Bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Bytes backed by frames: a lazy object's committed pages, every other kind's length.
    pub fn committed(&self) -> usize {
        self.lazy.as_ref().map_or(self.len, |pages| pages.committed() * PAGE)
    }

    /// Committed bytes of `[offset, offset + len)` — what a region over the object costs. A
    /// region over the whole object reads the running count (no page walk: the growth report
    /// measures every space at each reap, gpud's surfaces alone 130 000 pages).
    pub fn committed_in(&self, offset: usize, len: usize) -> usize {
        match &self.lazy {
            Some(pages) if offset == 0 && len >= self.len => pages.committed() * PAGE,
            Some(pages) => pages.committed_in(offset / PAGE, len.div_ceil(PAGE)) * PAGE,
            None => len,
        }
    }

    /// The physically contiguous runs `(pa, len)` in object order — adjacent blocks are one run
    /// (a trimmed contiguous object is several blocks and still one base).
    pub fn runs(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        let fixed = (self.kind == VmoKind::Fixed).then_some(self.fixed);
        let mut i = 0;
        let blocks = core::iter::from_fn(move || {
            let first = self.blocks.get(i)?;
            let (pa, mut len) = (first.base as usize, first.size() as usize);
            i += 1;
            while let Some(next) = self.blocks.get(i).filter(|b| b.base as usize == pa + len) {
                len += next.size() as usize;
                i += 1;
            }
            Some((pa, len))
        });
        blocks.chain(fixed)
    }

    /// The physical base when the object is ONE run (`Contiguous`, `Fixed`,
    /// or an `Anon` object that happened to fit one block).
    pub fn first_pa(&self) -> Option<usize> {
        let mut runs = self.runs();
        let (pa, _) = runs.next()?;
        runs.next().is_none().then_some(pa)
    }

    /// The physical address of byte `offset` (of a lazy object: only where committed).
    pub fn translate(&self, offset: usize) -> Option<usize> {
        if let Some(pages) = &self.lazy {
            return pages.frame(offset / PAGE).map(|pa| pa as usize + offset % PAGE);
        }
        let mut at = 0usize;
        for (pa, len) in self.runs() {
            if offset < at + len {
                return Some(pa + (offset - at));
            }
            at += len;
        }
        None
    }
}

struct VmoTable {
    objects: Vec<Option<VmoObject>>,
}

static TABLE: TableLock<VmoTable> = TableLock::new(VmoTable { objects: Vec::new() });

impl VmoTable {
    /// Stores the object in the lowest free slot, taking it out of `object` only then. Refused —
    /// no heap above the reserve for the table's growth (ADR-0093, ADR-0091) — it stays with the
    /// caller, so its frames can go back (returned inside the error it made every result 224
    /// bytes wide). The object's first capability (`read_only`: of an object no writer ever
    /// had) is made in the same hold: an object is never in the table held by nobody, so
    /// whoever judges an object nobody holds and nobody maps never meets one being born.
    fn insert(
        &mut self,
        object: &mut Option<VmoObject>,
        read_only: bool,
    ) -> Result<Born, VmoError> {
        let index = match self.objects.iter().position(Option::is_none) {
            Some(i) => i,
            None => {
                // The table grows with the objects, doubling, bounded by the pool.
                let more = self.objects.capacity().max(8);
                if self.objects.len() == self.objects.capacity()
                    && (!crate::kheap::admits(more * core::mem::size_of::<Option<VmoObject>>())
                        || self.objects.try_reserve(more).is_err())
                {
                    return Err(VmoError::HeapExhausted);
                }
                self.objects.push(None);
                self.objects.len() - 1
            }
        };
        let id = index as VmoId + 1;
        // The header first: refused by the heap, the object stays with the caller too.
        let len = object.as_ref().map_or(0, VmoObject::len);
        let held = HeldCounts::vmo(id, len).ok_or(VmoError::HeapExhausted)?;
        let first = if read_only {
            held.mint_read_only(Rights::MAP).ok_or(VmoError::NoSuchObject)?
        } else {
            held.mint(Rights::MAP)
        };
        self.objects[index] = object.take().map(|object| VmoObject { held: Some(held), ..object });
        Ok((id, first))
    }

    fn get(&self, id: VmoId) -> Option<&VmoObject> {
        self.objects.get(id.checked_sub(1)? as usize)?.as_ref()
    }

    fn get_mut(&mut self, id: VmoId) -> Option<&mut VmoObject> {
        self.objects.get_mut(id.checked_sub(1)? as usize)?.as_mut()
    }
}

/// A new object of `len` bytes (page-aligned, non-zero) that `reach` can
/// address (`DmaReach::ALL`: any frame), frames NOT zeroed — the caller zeroes
/// with no lock held (`zero`). Born held (`Born`).
pub fn create(len: usize, kind: VmoKind, reach: &DmaReach) -> Result<Born, VmoError> {
    if len == 0 || len % FRAME_SIZE as usize != 0 || kind == VmoKind::Fixed {
        return Err(VmoError::BadLength);
    }
    if max_object_bytes().is_some_and(|max| len > max) {
        return Err(VmoError::BadLength);
    }
    if kind == VmoKind::Lazy {
        // No frame now: each page takes one when it is first touched (`commit`) — from inside
        // the device's reach when the object is made for one (ADR-0078).
        let pages = Some(LazyPages::try_new(len / PAGE).ok_or(VmoError::HeapExhausted)?);
        let within = Within::of(reach);
        let object = VmoObject {
            kind,
            len,
            blocks: Vec::new(),
            fixed: (0, 0),
            lazy: pages,
            within,
            sealed: false,
            device_named: false,
            purgeable: false,
            purged: false,
            clean_block: 0,
            held: None,
        };
        // A refused lazy object holds no frame: its words go with it.
        return TABLE.lock().insert(&mut Some(object), false);
    }
    let mut windows = [(0u64, 0u64); MAX_DMA_WINDOWS];
    let mut n = 0;
    for range in reach.cpu_ranges() {
        windows[n] = range;
        n += 1;
    }
    let within = (!reach.is_all()).then_some(&windows[..n]);
    let blocks = match kind {
        VmoKind::Anon => match within {
            None => frame_pool::alloc_bytes(len),
            Some(windows) => frame_pool::alloc_bytes_within(len, windows),
        }
        .map_err(|err| match err {
            FrameError::NoHeap => VmoError::HeapExhausted,
            _ => VmoError::Exhausted,
        })?,
        VmoKind::Contiguous => {
            let pages = len / FRAME_SIZE as usize;
            let order = pages.next_power_of_two().trailing_zeros() as u8;
            if order > MAX_ORDER {
                return Err(VmoError::BadLength);
            }
            let block = match within {
                None => frame_pool::alloc(order),
                Some(windows) => frame_pool::alloc_within(order, windows),
            };
            match block {
                // The block is the next power of two: keep the object's frames, return the
                // rest at once (TASK-0333 6b-1 — it was held and counted nowhere).
                Ok(block) => frame_pool::trim(block, pages),
                Err(FrameError::BadOrder) => return Err(VmoError::BadLength),
                Err(_) => return Err(VmoError::Exhausted),
            }
        }
        VmoKind::Fixed | VmoKind::Lazy => unreachable!(),
    };
    let mut object = Some(VmoObject {
        kind,
        len,
        blocks,
        fixed: (0, 0),
        lazy: None,
        within: None,
        sealed: false,
        device_named: false,
        purgeable: false,
        purged: false,
        clean_block: 0,
        held: None,
    });
    let result = TABLE.lock().insert(&mut object, false);
    if let Some(refused) = object {
        // The table refused it: the frames go back before the error leaves.
        frame_pool::free_blocks(&refused.blocks);
    }
    result
}

/// An object over frames the pool does not own (`base`/`len` page-aligned). Born held.
pub fn adopt_fixed(base: usize, len: usize) -> Result<Born, VmoError> {
    if len == 0 || len % FRAME_SIZE as usize != 0 || base % FRAME_SIZE as usize != 0 {
        return Err(VmoError::BadLength);
    }
    let object = VmoObject {
        kind: VmoKind::Fixed,
        len,
        blocks: Vec::new(),
        fixed: (base, len),
        lazy: None,
        within: None,
        sealed: false,
        device_named: false,
        purgeable: false,
        purged: false,
        clean_block: 0,
        held: None,
    };
    // A fixed object's frames are not the pool's: nothing goes back on a refusal.
    TABLE.lock().insert(&mut Some(object), false)
}

/// An object over frames the pool does not own, SEALED from birth (ADR-0081): the bootfs the
/// loader placed and verified beside the kernel — no writer ever existed, so every file of it
/// is an image a process may run from. The kernel's own call; `seal` keeps refusing a fixed
/// object, because a fixed object made of a device window or the tree has a writer outside.
/// Born held by a read-only capability — the kernel's own, which it keeps (`bootfs`).
pub fn adopt_sealed(base: usize, len: usize) -> Result<Born, VmoError> {
    if len == 0 || len % FRAME_SIZE as usize != 0 || base % FRAME_SIZE as usize != 0 {
        return Err(VmoError::BadLength);
    }
    let object = VmoObject {
        kind: VmoKind::Fixed,
        len,
        blocks: Vec::new(),
        fixed: (base, len),
        lazy: None,
        within: None,
        sealed: true,
        device_named: false,
        purgeable: false,
        purged: false,
        clean_block: 0,
        held: None,
    };
    TABLE.lock().insert(&mut Some(object), true)
}

/// Destroy: the frames go back to the pool (a `Fixed` object frees nothing)
/// and the id becomes free. That no capability is held to it and no address space maps it the
/// object answers itself, in this hold of the table (`Held`, `Mapped` otherwise) — so neither
/// a capability nor a mapping outlives its object to name the next one that gets the id.
/// Answers the object's bytes (0 for a `Fixed` one): what the caller counts is what this hold
/// took, not what a second look at the id would find.
pub fn destroy(id: VmoId) -> Result<usize, VmoError> {
    let object = {
        let mut table = TABLE.lock();
        let slot = table.objects.get_mut(id.checked_sub(1).ok_or(VmoError::NoSuchObject)? as usize);
        let slot = slot.ok_or(VmoError::NoSuchObject)?;
        let counts = slot.as_ref().and_then(|o| o.held.as_ref());
        if counts.is_some_and(|held| held.any() > 0) {
            return Err(VmoError::Held);
        }
        if counts.is_some_and(|held| held.mapped() > 0) {
            return Err(VmoError::Mapped);
        }
        slot.take().ok_or(VmoError::NoSuchObject)?
    };
    frame_pool::free_blocks(&object.blocks);
    if let Some(pages) = &object.lazy {
        // The committed pages' frames and the store's (ADR-0087: the records die with it).
        for pa in pages.frames().chain(pages.store_frames()) {
            frame_pool::free(Block { base: pa, order: 0 });
        }
        // The pager's slots of its swapped pages are told free (ADR-0088).
        let slots: Vec<u32> = pages.swapped_slots().collect();
        crate::mm::swap::dropped(id, &slots);
    }
    Ok(if object.kind == VmoKind::Fixed { 0 } else { object.len })
}

/// Run `f` over the live object.
pub fn with<R>(id: VmoId, f: impl FnOnce(&VmoObject) -> R) -> Option<R> {
    TABLE.lock().get(id).map(f)
}

/// The header of the live object `id`: the capabilities held to it (RFC-0102).
pub fn held_counts(id: VmoId) -> Option<Arc<HeldCounts>> {
    with(id, |o| o.held.clone()).flatten()
}

/// How many capabilities are held to the live object `id` (0 for one that is gone).
pub fn held(id: VmoId) -> usize {
    held_counts(id).map_or(0, |counts| counts.any())
}

/// The object's runs, copied out (syscalls walk them without the lock). `None` for a lazy
/// object too: it has pages, not runs — `span` answers for every kind.
pub fn runs(id: VmoId) -> Option<Vec<(usize, usize)>> {
    with(id, |o| o.lazy.is_none().then(|| o.runs().collect())).flatten()
}

/// Whether the live object `id` commits its pages on first touch (ADR-0077).
pub fn is_lazy(id: VmoId) -> bool {
    with(id, |o| o.kind == VmoKind::Lazy).unwrap_or(false)
}

/// The physical address of byte `offset`, or `None` past the end.
pub fn translate(id: VmoId, offset: usize) -> Option<usize> {
    with(id, |o| o.translate(offset)).flatten()
}

/// The physical base when the object is one run.
pub fn first_pa(id: VmoId) -> Option<usize> {
    with(id, VmoObject::first_pa).flatten()
}

/// Zero every run through the direct map. Called with the BKL dropped and
/// no table lock held — the runs are copied first.
pub fn zero(id: VmoId) {
    let Some(runs) = runs(id) else { return };
    let clean = clean::block(id);
    for (pa, len) in runs {
        // SAFETY: frames this object owns exclusively until its capability
        // exists; nobody else can reach them.
        unsafe {
            crate::smp::tlb::zero_bytes_polled(crate::phys::phys_to_virt(pa) as *mut u8, len);
        }
        // ADR-0092: zeroes a non-coherent device reads are in memory, not only in a cache.
        clean::range_polled(pa, len, clean);
    }
}

/// `(live objects, bytes they span, bytes of the contiguous ones, bytes committed)` — a lazy
/// object spans its length and commits its touched pages; every other kind commits its length.
pub fn stats() -> (usize, usize, usize, usize) {
    let table = TABLE.lock();
    let live = table.objects.iter().flatten();
    let contiguous = live.clone().filter(|o| o.kind == VmoKind::Contiguous).map(VmoObject::len);
    let committed = live.clone().map(VmoObject::committed).sum();
    (live.clone().count(), live.map(VmoObject::len).sum(), contiguous.sum(), committed)
}

/// Committed bytes of `[offset, offset + len)` of `id` (`len` itself for an eager object, 0 for
/// one that is gone) — what a region over it costs its space.
pub fn committed_in(id: VmoId, offset: usize, len: usize) -> usize {
    with(id, |o| o.committed_in(offset, len)).unwrap_or(0)
}

/// Whether the live object `id` is one physically contiguous (DMA) block.
pub fn is_contiguous(id: VmoId) -> bool {
    with(id, |o| o.kind == VmoKind::Contiguous).unwrap_or(false)
}
