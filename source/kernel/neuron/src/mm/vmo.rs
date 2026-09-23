// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the page-backed VMO (TASK-0286 P3a, RFC-0098 C4). An object is a
//! list of physically contiguous blocks from the frame pool: `Anon` takes the
//! largest blocks that fit (greedy, so a 2 MiB-aligned run stays a superpage
//! when mapped), `Contiguous` takes ONE block — the DMA masters' kind (virtio
//! queues, framebuffers, app surfaces the GPU scans out); its physical base is
//! what `cap_query` reports, an `Anon` object reports none. `Fixed` wraps
//! frames that never came from the pool (the tree, a kernel data page) and are
//! never returned. Capabilities carry the id (`Vmo { id, len }`,
//! `VmoRo { id, len }`); this table is the ONE owner of the frames behind them
//! — `VmoPool` and the fixed arena are gone.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: block placement is host-tested in `crate::frames`; the object
//!   paths are proven by the VMO selftests and every service image
//! INVARIANTS: an id names at most one live object; frames of a destroyed
//!   object are back in the pool before the id is reusable; a `Fixed` object
//!   never frees; the table lock is a leaf (never held across a memset).

use alloc::vec::Vec;

use super::frame_pool;
use crate::frames::{Block, FrameError, FRAME_SIZE, MAX_ORDER};

#[cfg(debug_assertions)]
type TableLock<T> = crate::sync::dbg_mutex::DbgMutex<T>;
#[cfg(not(debug_assertions))]
type TableLock<T> = spin::Mutex<T>;

/// A live object's id: never 0, the table index plus one.
pub type VmoId = u32;

/// Objects the table holds at most (a slot is 40 bytes plus its block list).
pub const MAX_OBJECTS: usize = 4096;

/// How an object is backed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmoKind {
    /// Pool blocks, largest-first; no physical base is promised.
    Anon,
    /// One pool block: physically contiguous, `cap_query` names its base.
    Contiguous,
    /// Frames outside the pool (never freed).
    Fixed,
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
}

/// One object.
pub struct VmoObject {
    kind: VmoKind,
    len: usize,
    blocks: Vec<Block>,
    fixed: (usize, usize),
}

impl VmoObject {
    /// Bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// The physically contiguous runs `(pa, len)` in object order.
    pub fn runs(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        let fixed = (self.kind == VmoKind::Fixed).then_some(self.fixed);
        self.blocks.iter().map(|b| (b.base as usize, b.size() as usize)).chain(fixed)
    }

    /// The physical base when the object is ONE run (`Contiguous`, `Fixed`,
    /// or an `Anon` object that happened to fit one block).
    pub fn first_pa(&self) -> Option<usize> {
        let mut runs = self.runs();
        let (pa, _) = runs.next()?;
        runs.next().is_none().then_some(pa)
    }

    /// The physical address of byte `offset`.
    pub fn translate(&self, offset: usize) -> Option<usize> {
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
}

/// A new object of `len` bytes (page-aligned, non-zero), frames NOT zeroed —
/// the caller zeroes with the BKL dropped (`zero`).
pub fn create(len: usize, kind: VmoKind) -> Result<VmoId, VmoError> {
    if len == 0 || len % FRAME_SIZE as usize != 0 || kind == VmoKind::Fixed {
        return Err(VmoError::BadLength);
    }
    let blocks = match kind {
        VmoKind::Anon => frame_pool::alloc_bytes(len).map_err(|_| VmoError::Exhausted)?,
        VmoKind::Contiguous => {
            let pages = len / FRAME_SIZE as usize;
            let order = pages.next_power_of_two().trailing_zeros() as u8;
            if order > MAX_ORDER {
                return Err(VmoError::BadLength);
            }
            match frame_pool::alloc(order) {
                Ok(block) => alloc::vec![block],
                Err(FrameError::BadOrder) => return Err(VmoError::BadLength),
                Err(_) => return Err(VmoError::Exhausted),
            }
        }
        VmoKind::Fixed => unreachable!(),
    };
    let result =
        TABLE.lock().insert(VmoObject { kind, len, blocks: blocks.clone(), fixed: (0, 0) });
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
    let object = VmoObject { kind: VmoKind::Fixed, len, blocks: Vec::new(), fixed: (base, len) };
    TABLE.lock().insert(object)
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
    Ok(())
}

/// Run `f` over the live object.
pub fn with<R>(id: VmoId, f: impl FnOnce(&VmoObject) -> R) -> Option<R> {
    TABLE.lock().get(id).map(f)
}

/// The object's runs, copied out (syscalls walk them without the lock).
pub fn runs(id: VmoId) -> Option<Vec<(usize, usize)>> {
    with(id, |o| o.runs().collect())
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

/// `(live objects, bytes they hold)`.
pub fn stats() -> (usize, usize) {
    let table = TABLE.lock();
    let live = table.objects.iter().flatten();
    (live.clone().count(), live.map(VmoObject::len).sum())
}
