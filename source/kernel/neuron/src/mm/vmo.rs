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

use alloc::vec::Vec;

use super::frame_pool;
use crate::dma_reach::{DmaReach, MAX_DMA_WINDOWS};
use crate::frames::{Block, FrameError, FRAME_SIZE, MAX_ORDER};
use crate::lazy::{CommitError, LazyPages, PageState, PAGE};

#[cfg(debug_assertions)]
type TableLock<T> = crate::sync::dbg_mutex::DbgMutex<T>;
#[cfg(not(debug_assertions))]
type TableLock<T> = spin::Mutex<T>;

/// A live object's id: never 0, the table index plus one.
pub type VmoId = u32;

/// Objects between a create's phases (RFC-0100 §1): the lifetime rule leaves them alone.
mod creating;
pub use creating::{is_creating, set_creating};
/// Purgeable objects (RFC-0100 §4): the unlock, the lock, the purge at `warn`.
pub mod purge;
/// Squeezed pages (RFC-0100 §5, ADR-0087): a page recorded, a page brought back.
pub mod squeeze;

/// Objects the table holds at most (a slot is 40 bytes plus its block list).
pub const MAX_OBJECTS: usize = 4096;

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
    /// `MAX_OBJECTS` live objects.
    TableFull,
    /// No live object has this id.
    NoSuchObject,
    /// A squeezed page's record did not decode to one page (ADR-0087): the access fails, the
    /// record stays.
    Damaged,
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
    fn insert(&mut self, object: VmoObject) -> Result<VmoId, VmoError> {
        let index = match self.objects.iter().position(Option::is_none) {
            Some(i) => i,
            None if self.objects.len() < MAX_OBJECTS => {
                self.objects.push(None);
                self.objects.len() - 1
            }
            None => return Err(VmoError::TableFull),
        };
        self.objects[index] = Some(object);
        Ok(index as VmoId + 1)
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
/// with the BKL dropped (`zero`).
pub fn create(len: usize, kind: VmoKind, reach: &DmaReach) -> Result<VmoId, VmoError> {
    if len == 0 || len % FRAME_SIZE as usize != 0 || kind == VmoKind::Fixed {
        return Err(VmoError::BadLength);
    }
    if kind == VmoKind::Lazy {
        // No frame now: each page takes one when it is first touched (`commit`) — from inside
        // the device's reach when the object is made for one (ADR-0078).
        let pages = Some(LazyPages::new(len / PAGE));
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
        };
        return TABLE.lock().insert(object);
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
        .map_err(|_| VmoError::Exhausted)?,
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
    let result = TABLE.lock().insert(VmoObject {
        kind,
        len,
        blocks: blocks.clone(),
        fixed: (0, 0),
        lazy: None,
        within: None,
        sealed: false,
        device_named: false,
        purgeable: false,
        purged: false,
    });
    if result.is_err() {
        // The table is full: the frames go back before the error leaves.
        frame_pool::free_blocks(&blocks);
    }
    result
}

/// An object over frames the pool does not own (`base`/`len` page-aligned).
pub fn adopt_fixed(base: usize, len: usize) -> Result<VmoId, VmoError> {
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
    };
    TABLE.lock().insert(object)
}

/// An object over frames the pool does not own, SEALED from birth (ADR-0081): the bootfs the
/// loader placed and verified beside the kernel — no writer ever existed, so every file of it
/// is an image a process may run from. The kernel's own call; `seal` keeps refusing a fixed
/// object, because a fixed object made of a device window or the tree has a writer outside.
pub fn adopt_sealed(base: usize, len: usize) -> Result<VmoId, VmoError> {
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
    };
    TABLE.lock().insert(object)
}

/// Why a seal was refused (ADR-0080).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealError {
    /// No live object has this id.
    NoSuchObject,
    /// A lazy or fixed object: its pages are not all the object's own, committed frames.
    Kind,
    /// A device was told its runs and may write them at any time.
    DeviceNamed,
}

/// Seal `id` (ADR-0080): the object has no writer for the rest of its life. Idempotent. The
/// caller has proven no address space maps it writably (`vm_ops::any_space_maps_vmo_writable`).
pub fn seal(id: VmoId) -> Result<(), SealError> {
    let mut table = TABLE.lock();
    let object = table.get_mut(id).ok_or(SealError::NoSuchObject)?;
    if object.lazy.is_some() || object.kind == VmoKind::Fixed {
        return Err(SealError::Kind);
    }
    if object.device_named {
        return Err(SealError::DeviceNamed);
    }
    object.sealed = true;
    Ok(())
}

/// Whether the live object `id` is sealed (ADR-0080).
pub fn is_sealed(id: VmoId) -> bool {
    with(id, |o| o.sealed).unwrap_or(false)
}

/// Record that a device was told `id`'s runs (`vmo_runs`): never sealable from now on.
pub fn mark_device_named(id: VmoId) {
    if let Some(object) = TABLE.lock().get_mut(id) {
        object.device_named = true;
    }
}

/// Destroy: the frames go back to the pool (a `Fixed` object frees nothing)
/// and the id becomes free. The caller has proven nobody maps or names it.
pub fn destroy(id: VmoId) -> Result<(), VmoError> {
    let object = {
        let mut table = TABLE.lock();
        let slot = table.objects.get_mut(id.checked_sub(1).ok_or(VmoError::NoSuchObject)? as usize);
        slot.and_then(Option::take).ok_or(VmoError::NoSuchObject)?
    };
    frame_pool::free_blocks(&object.blocks);
    if let Some(pages) = &object.lazy {
        // The committed pages' frames and the store's (ADR-0087: the records die with it).
        for pa in pages.frames().chain(pages.store_frames()) {
            frame_pool::free(Block { base: pa, order: 0 });
        }
    }
    Ok(())
}

/// Run `f` over the live object.
pub fn with<R>(id: VmoId, f: impl FnOnce(&VmoObject) -> R) -> Option<R> {
    TABLE.lock().get(id).map(f)
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

/// Page `index` of `id`, committed now if it was not: a frame from the pool, zeroed before it
/// is entered — outside the table lock, which is a leaf and never held across a memset. A
/// squeezed page is brought back instead (ADR-0087, `squeeze::inflate`). A page whose word moved
/// on between the look and the act is looked at once more (under the kernel lock nothing moves
/// it). Answers the page's physical address; an eager object answers its translation.
pub fn commit(id: VmoId, index: usize) -> Result<usize, VmoError> {
    for _ in 0..2 {
        let (state, within) = with(id, |o| match &o.lazy {
            Some(pages) => pages.state(index).map(|s| (s, o.within)).ok_or(VmoError::BadLength),
            None => o
                .translate(index * PAGE)
                .map(|pa| (PageState::Frame(pa as u64), None))
                .ok_or(VmoError::BadLength),
        })
        .ok_or(VmoError::NoSuchObject)??;
        let done = match state {
            PageState::Frame(pa) => Some(pa as usize),
            PageState::Record(_) => squeeze::inflate(id, index, within)?,
            PageState::Absent => commit_zeroed(id, index, within)?,
        };
        if let Some(pa) = done {
            return Ok(pa);
        }
    }
    // A page that moved on twice under the kernel lock: the table is not what it says.
    Err(VmoError::Damaged)
}

/// Commit absent page `index` to a zeroed frame; `None` when it turned out to be a record.
fn commit_zeroed(
    id: VmoId,
    index: usize,
    within: Option<Within>,
) -> Result<Option<usize>, VmoError> {
    let block = match within {
        None => frame_pool::alloc(0),
        Some(within) => frame_pool::alloc_within(0, within.ranges()),
    }
    .map_err(|_| VmoError::Exhausted)?;
    // SAFETY: a frame just taken from the pool, reachable by nobody else until it is entered.
    unsafe {
        core::ptr::write_bytes(crate::phys::phys_to_virt(block.base as usize) as *mut u8, 0, PAGE);
    }
    let entered = match TABLE.lock().get_mut(id).and_then(|o| o.lazy.as_mut()) {
        Some(pages) => pages.commit(index, block.base).map_err(Some),
        None => Err(None),
    };
    if entered.is_err() {
        frame_pool::free(block);
    }
    match entered {
        Ok(()) => Ok(Some(block.base as usize)),
        Err(Some(CommitError::Committed(pa))) => Ok(Some(pa as usize)),
        // Squeezed between the look and the commit: the caller looks again and inflates it.
        Err(Some(CommitError::Squeezed)) => Ok(None),
        Err(Some(CommitError::OutOfRange)) => Err(VmoError::BadLength),
        // The object died between the look and the commit.
        Err(None) => Err(VmoError::NoSuchObject),
    }
}

/// Lazy page `index` of `id` for a read: its frame, `None` when absent (zero) — a squeezed page
/// is content, never zero: it is brought back first (ADR-0087).
fn resident(id: VmoId, index: usize) -> Result<Option<usize>, VmoError> {
    match with(id, |o| o.lazy.as_ref().and_then(|p| p.state(index)))
        .ok_or(VmoError::NoSuchObject)?
    {
        Some(PageState::Frame(pa)) => Ok(Some(pa as usize)),
        Some(PageState::Absent) => Ok(None),
        Some(PageState::Record(_)) => commit(id, index).map(Some),
        None => Err(VmoError::BadLength),
    }
}

/// The bytes `[offset, offset + len)` of `id` as pieces `(pa, len)` in order: the runs of an
/// eager object, the pages of a lazy one — `None` where a lazy page is absent. With
/// `commit_pages` every lazy page of the span is committed first (a write, a device's runs), so
/// no piece is `None`; a squeezed page is brought back either way.
pub fn span(
    id: VmoId,
    offset: usize,
    len: usize,
    commit_pages: bool,
) -> Result<Vec<(Option<usize>, usize)>, VmoError> {
    let end = offset.checked_add(len).ok_or(VmoError::BadLength)?;
    let (lazy, object_len) =
        with(id, |o| (o.lazy.is_some(), o.len)).ok_or(VmoError::NoSuchObject)?;
    if end > object_len {
        return Err(VmoError::BadLength);
    }
    let mut out = Vec::new();
    if !lazy {
        let mut at = 0usize;
        for (pa, run_len) in runs(id).ok_or(VmoError::NoSuchObject)? {
            let (lo, hi) = (offset.max(at), end.min(at + run_len));
            if lo < hi {
                out.push((Some(pa + (lo - at)), hi - lo));
            }
            at += run_len;
        }
        return Ok(out);
    }
    let mut at = offset;
    while at < end {
        let piece = (end - at).min(PAGE - at % PAGE);
        let base =
            if commit_pages { Some(commit(id, at / PAGE)?) } else { resident(id, at / PAGE)? };
        out.push((base.map(|pa| pa + at % PAGE), piece));
        at += piece;
    }
    Ok(out)
}

/// `[offset, offset + len)` of a lazy object as a device is told it (RFC-0100 §2, ADR-0078):
/// every page committed first — a device cannot fault — then the pieces merged into runs, in
/// range order. At most `MAX_NAMED_PAGES` pages at once: the commits run under the kernel lock.
pub fn named_runs(id: VmoId, offset: usize, len: usize) -> Result<Vec<(usize, usize)>, VmoError> {
    if !crate::lazy::nameable(offset, len) {
        return Err(VmoError::BadLength);
    }
    let pieces = span(id, offset, len, true)?;
    Ok(crate::lazy::merge_runs(pieces.into_iter().filter_map(|(pa, len)| Some((pa?, len)))))
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
    for (pa, len) in runs {
        // SAFETY: frames this object owns exclusively until its capability
        // exists; nobody else can reach them.
        unsafe {
            crate::smp::tlb::zero_bytes_polled(crate::phys::phys_to_virt(pa) as *mut u8, len);
        }
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

/// Whether the live object `id` is over frames the pool does not own (`destroy` frees nothing).
pub fn is_fixed(id: VmoId) -> bool {
    with(id, |o| o.kind == VmoKind::Fixed).unwrap_or(false)
}
