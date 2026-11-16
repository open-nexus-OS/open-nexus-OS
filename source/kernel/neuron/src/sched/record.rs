// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a task's scheduling record — its saved frame and who holds it (TASK-0363 P2 S2).
//! A record lives at a fixed address for the life of the kernel: the first 64 in a static block,
//! the rest in blocks of 64 allocated as pids are handed out and never moved or freed (pids are
//! never reused). So the hart that takes a task off its queue reaches the task's frame without
//! the kernel lock and without the task table, which moves when it grows (S3's dispatch).
//! The frame is guarded by its owner word, not by a lock: written before the enqueue, read after
//! the pick, touched by nobody in between (`run_queue::Owner`). Every access checks that rule and
//! names the call site that broke it — the tripwire S3 needs before the lock goes.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the owner rule in `run_queue_tests.rs` (host); every QEMU lane and the board
//!   ladder fail on a `KINIT: FAIL frame owner` line
//! INVARIANTS: a record's address never changes; a pid's record is claimed once, before its task
//!   exists; the frame is touched only by a hart `Owner::may_touch` admits

use core::cell::UnsafeCell;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use crate::run_queue::Owner;
use crate::trap::fp::Held;
use crate::trap::TrapFrame;
use crate::types::Pid;

/// Records a block holds.
const BLOCK: usize = 64;
/// Blocks the directory holds: 65 536 tasks — a spawn past it is refused (`claim`).
const BLOCKS: usize = 1024;

/// One task's scheduling record.
pub struct Record {
    owner: AtomicU32,
    frame: UnsafeCell<TrapFrame>,
    /// The floating-point and vector states the task owns (TASK-0362) — held like the frame.
    fp: UnsafeCell<Held>,
    /// The task's address space as its dispatch needs it (S3: the switch without the kernel
    /// lock): the raw handle (0 = none), its `satp` and its cache-maintenance right. Written by
    /// the address-space manager where it records the task as the space's owner, read by the
    /// hart that dispatches the task.
    space: AtomicU32,
    satp: AtomicUsize,
    cache: AtomicBool,
}

// SAFETY: the frame and the floating-point states are reached only through their accessors,
// whose callers hold them by the owner protocol: the kernel lock for a free task's, the running
// hart for its own — never two harts at once (`Owner::may_touch`, checked on every access). The
// rest are atomics.
unsafe impl Sync for Record {}

/// A task's address space as a dispatch switches to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceView {
    pub raw: u32,
    pub satp: usize,
    pub cache: bool,
}

impl Record {
    const fn new() -> Self {
        Self {
            owner: AtomicU32::new(0),
            frame: UnsafeCell::new(TrapFrame::EMPTY),
            fp: UnsafeCell::new(Held::NONE),
            space: AtomicU32::new(0),
            satp: AtomicUsize::new(0),
            cache: AtomicBool::new(false),
        }
    }

    /// The task now owns space `view` (the manager's `attach`, under the kernel lock).
    pub fn set_space(&self, view: SpaceView) {
        self.satp.store(view.satp, Ordering::Relaxed);
        self.cache.store(view.cache, Ordering::Relaxed);
        self.space.store(view.raw, Ordering::Release);
    }

    /// The task no longer owns space `raw` (the manager's `detach`).
    pub fn clear_space(&self, raw: u32) {
        let _ = self.space.compare_exchange(raw, 0, Ordering::AcqRel, Ordering::Acquire);
    }

    /// The task's space was granted cache maintenance (ADR-0079).
    pub fn grant_cache(&self) {
        self.cache.store(true, Ordering::Release);
    }

    /// The task's space as its dispatch switches to it; `None` without one.
    pub fn space(&self) -> Option<SpaceView> {
        let raw = self.space.load(Ordering::Acquire);
        (raw != 0).then(|| SpaceView {
            raw,
            satp: self.satp.load(Ordering::Relaxed),
            cache: self.cache.load(Ordering::Acquire),
        })
    }

    /// The task's floating-point states (held as the frame is).
    #[track_caller]
    pub fn fp(&self) -> &Held {
        self.check(false);
        // SAFETY: the holder rule, as for `frame`.
        unsafe { &*self.fp.get() }
    }

    /// The task's floating-point states if this hart holds the record, `None` otherwise — for
    /// `TaskTable::set_current`, which a dispatch calls with the task it picked and the kernel
    /// selftests call to act as a task that is still queued (they run none of its code, so it
    /// has no floating-point context there; the frame rule still names a real breach).
    pub fn fp_if_held(&self) -> Option<&Held> {
        let hart = crate::smp::cpu_current_id().as_index() as u16;
        let held = Owner::decode(self.owner.load(Ordering::Acquire))
            .is_some_and(|owner| owner.may_touch(hart));
        // SAFETY: the holder rule, as for `frame` — checked just above.
        held.then(|| unsafe { &*self.fp.get() })
    }

    /// The task's floating-point states, to change (held as the frame is).
    #[track_caller]
    #[allow(clippy::mut_from_ref)]
    pub fn fp_mut(&self) -> &mut Held {
        self.check(true);
        // SAFETY: the holder rule, as for `frame_mut`.
        unsafe { &mut *self.fp.get() }
    }

    /// The scheduler hands the record on (under the queue lock of the hart concerned).
    pub fn set_owner(&self, owner: Owner) {
        self.owner.store(owner.encode(), Ordering::Release);
    }

    /// Hands the record from `from` to `to` if `from` still holds it.
    pub fn pass(&self, from: Owner, to: Owner) -> bool {
        self.owner
            .compare_exchange(from.encode(), to.encode(), Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// The saved frame. The caller is the record's holder (`Owner::may_touch`).
    #[track_caller]
    pub fn frame(&self) -> &TrapFrame {
        self.check(false);
        // SAFETY: the holder rule — no hart writes this frame while the holder reads it.
        unsafe { &*self.frame.get() }
    }

    /// The saved frame, to change. The caller is the record's holder, and its `&mut Task` (or
    /// the queue state that made it the holder) is the only way it reached the record.
    #[track_caller]
    #[allow(clippy::mut_from_ref)]
    pub fn frame_mut(&self) -> &mut TrapFrame {
        self.check(true);
        // SAFETY: the holder rule — exactly one hart holds the record, and it borrows the frame
        // through the one `&mut Task` the kernel lock grants (or as the task's running hart).
        unsafe { &mut *self.frame.get() }
    }

    /// Names a call site that touches a frame it does not hold: the first few, FAIL-shaped, so
    /// the lane or the board ladder that meets one goes red.
    #[track_caller]
    fn check(&self, write: bool) {
        let raw = self.owner.load(Ordering::Acquire);
        let hart = crate::smp::cpu_current_id().as_index() as u16;
        if Owner::decode(raw).is_some_and(|owner| owner.may_touch(hart)) {
            return;
        }
        static NAMED: AtomicUsize = AtomicUsize::new(0);
        if NAMED.fetch_add(1, Ordering::Relaxed) < 8 {
            let at = core::panic::Location::caller();
            log_error!(
                target: "sched",
                "KINIT: FAIL frame owner (owner={:?} hart={} write={} at={}:{})",
                Owner::decode(raw),
                hart,
                write,
                at.file(),
                at.line()
            );
        }
    }
}

/// Pids `0..64`: the bootstrap task and the first spawns, without an allocation.
static FIRST: [Record; BLOCK] = [const { Record::new() }; BLOCK];
/// Blocks `1..`: allocated by `claim`, never freed.
static BLOCKS_AT: [AtomicPtr<Record>; BLOCKS] = [const { AtomicPtr::new(null_mut()) }; BLOCKS];

/// The record of `pid`, if it was claimed (a pid past the directory, or one no task holds — a
/// selftest's probe id — has none).
pub fn get(pid: Pid) -> Option<&'static Record> {
    let (block, at) = (pid.as_index() / BLOCK, pid.as_index() % BLOCK);
    if block == 0 {
        return FIRST.get(at);
    }
    let base = BLOCKS_AT.get(block)?.load(Ordering::Acquire);
    if base.is_null() {
        return None;
    }
    // SAFETY: a non-null entry points at a leaked block of `BLOCK` records that is never freed
    // or moved; `at < BLOCK`.
    Some(unsafe { &*base.add(at) })
}

/// The bootstrap task's record (pid 0, the static block — nothing to allocate, nothing to fail).
pub fn bootstrap(frame: TrapFrame) -> &'static Record {
    let record = &FIRST[0];
    record.set_owner(Owner::Free);
    *record.frame_mut() = frame;
    record
}

/// A spawn could not get a record: the directory is full or the heap refused a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoRecord;

/// A spawn without a record is refused as one without memory (bounded, explicit).
impl From<NoRecord> for crate::task::SpawnError {
    fn from(_: NoRecord) -> Self {
        Self::OutOfMemory
    }
}

/// Claims the record of a pid handed out now (under the kernel lock, before its task exists):
/// free, holding `frame`.
pub fn claim(pid: Pid, frame: TrapFrame) -> Result<&'static Record, NoRecord> {
    let block = pid.as_index() / BLOCK;
    let slot = BLOCKS_AT.get(block).ok_or(NoRecord)?;
    if block != 0 && slot.load(Ordering::Acquire).is_null() {
        let mut fresh = alloc::vec::Vec::new();
        fresh.try_reserve_exact(BLOCK).map_err(|_| NoRecord)?;
        fresh.extend((0..BLOCK).map(|_| Record::new()));
        let base = alloc::boxed::Box::into_raw(fresh.into_boxed_slice()) as *mut Record;
        if slot.compare_exchange(null_mut(), base, Ordering::AcqRel, Ordering::Acquire).is_err() {
            // SAFETY: `base` came from `Box::into_raw` above and was published nowhere.
            drop(unsafe {
                alloc::boxed::Box::from_raw(core::ptr::slice_from_raw_parts_mut(base, BLOCK))
            });
        }
    }
    let record = get(pid).ok_or(NoRecord)?;
    record.set_owner(Owner::Free);
    *record.frame_mut() = frame;
    Ok(record)
}
