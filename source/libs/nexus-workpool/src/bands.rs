// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the bands of a pass painted on every hart the pool has (TASK-0367 P2) — the pool's
//! SAFE scoped entry. A caller hands its rows (`&mut [u8]`, rows `stride` bytes apart), a shared
//! context and a plain `fn`; the rows are cut into bands of whole rows, and every painter — the
//! pool's workers AND the caller — takes the next band from one atomic cursor until none is
//! left: no share is fixed, a busy hart takes fewer bands (ADR-0046: an entry whose outputs are
//! positional may hand out its chunks by load). The call joins before it returns, so the borrows
//! of the rows and the context cannot outlive it — the soundness of a scoped spawn. The unsafe
//! stays here: the callers (app-host) keep `unsafe_code` denied.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `bands_tests.rs` (host, miri): the shape bounds; scripted interleavings for 1..8
//!   painters (claims made before the caller starts, painted only in the join); real threads; the
//!   outcome of every start error and of a dead canary.
//! INVARIANTS:
//!   - Band `i` is bytes `[i * band_bytes, min((i + 1) * band_bytes, len))`: disjoint, in order,
//!     together the whole slice. The cursor hands each index out once; a claim is consumed by its
//!     paint (not `Clone`), so no band is painted twice and none is skipped.
//!   - The job lives on the caller's stack; a worker touches it only between waking on the job and
//!     its check-in. The call returns only after every worker checked in (`Crew::join`).
//!   - A retryable error (`Busy`, `Poisoned`, `NotReady`, a failed signal) comes only from
//!     `Crew::start`, before anything is published: the caller then takes every band through the
//!     same loop and the call returns `Ok(1)`. After a start the outcome is `Ok(painters)` or
//!     `Err(StackOverflow)` (after the join: the bands are complete; the pool is poisoned).
//!   - At most `MAX_BANDS` bands of at least `MIN_BAND_ROWS` rows (fewer rows: one band): a taller
//!     pass takes taller bands.
//!
//! ADR: docs/adr/0046-deterministic-parallel-compute-workpool.md

use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::pool::PoolError;

/// A band is at least this many rows (a pass of fewer rows is one band).
pub const MIN_BAND_ROWS: usize = 8;

/// A pass is cut into at most this many bands; a taller pass takes taller bands.
pub const MAX_BANDS: usize = 128;

/// How a pass's rows are cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shape {
    /// Rows per band (the last band may hold fewer).
    pub(crate) band_rows: usize,
    /// How many bands.
    pub(crate) bands: usize,
}

impl Shape {
    /// `len` bytes of rows `stride` bytes apart (the last row may be shorter than `stride`),
    /// asked in bands of `band_rows`: at least [`MIN_BAND_ROWS`] each (never more than the pass
    /// holds), at most [`MAX_BANDS`] of them. No rows: no band.
    pub(crate) const fn of(len: usize, stride: usize, band_rows: usize) -> Self {
        if len == 0 || stride == 0 {
            return Self { band_rows: 0, bands: 0 };
        }
        let rows = len.div_ceil(stride);
        let mut band_rows = band_rows;
        if band_rows < MIN_BAND_ROWS {
            band_rows = MIN_BAND_ROWS;
        }
        let floor = rows.div_ceil(MAX_BANDS);
        if band_rows < floor {
            band_rows = floor;
        }
        if band_rows > rows {
            band_rows = rows;
        }
        Self { band_rows, bands: rows.div_ceil(band_rows) }
    }
}

// The bound holds for any pass: a 64 Ki-row pass of one-row asks is still `MAX_BANDS` bands, and
// a short pass is one band.
const _: () = {
    assert!(MIN_BAND_ROWS > 0 && MAX_BANDS > 0);
    assert!(Shape::of(65_536 * 16, 16, 1).bands == MAX_BANDS);
    assert!(Shape::of(65_536 * 16, 16, 1).band_rows == 65_536 / MAX_BANDS);
    assert!(Shape::of(5 * 16, 16, 1).bands == 1);
};

/// The caller's context and painter, as a job refers to them (type-erased in [`Job`]).
struct Erased<'c, C> {
    ctx: &'c C,
    f: fn(&C, usize, &mut [u8]),
}

/// Paints one band through an [`Erased`] of the `C` it was instantiated for.
type PaintFn = unsafe fn(erased: *const (), first_row: usize, band: &mut [u8]);

/// # Safety
/// `erased` points to a live `Erased<'_, C>` of this very `C`.
unsafe fn call<C>(erased: *const (), first_row: usize, band: &mut [u8]) {
    // SAFETY: the caller's contract — `Job::new` pairs `call::<C>` with an `Erased<C>` that
    // outlives the job.
    let e = unsafe { &*erased.cast::<Erased<'_, C>>() };
    (e.f)(e.ctx, first_row, band);
}

/// One pass's bands, shared by reference between the caller and the workers for the call.
pub(crate) struct Job<'a> {
    /// The start of the caller's rows (from its exclusive borrow, held for `'a`).
    base: *mut u8,
    len: usize,
    band_bytes: usize,
    band_rows: usize,
    bands: usize,
    /// The next band to hand out.
    cursor: AtomicUsize,
    erased: *const (),
    paint: PaintFn,
    _rows: PhantomData<&'a mut [u8]>,
}

// SAFETY: a `Job` is shared by reference between the caller and the pool's workers for one call.
// Its cursor is atomic. The band slices it makes are disjoint (one index per claim, consumed by
// `paint`) and lie inside the exclusive borrow of the rows the job holds for `'a` (`u8` is
// `Send`). `erased` points at an `Erased<C>` whose `C: Sync` (`Job::new`'s bound), so `&C` may be
// read from any thread, and its `f` is a plain `fn`. Nothing else is written through a `&Job`.
unsafe impl Sync for Job<'_> {}

/// A band claimed from the cursor: its painter's alone, painted once (consumed by `Job::paint`).
pub(crate) struct Claim(usize);

impl<'a> Job<'a> {
    fn new<C: Sync>(
        rows: &'a mut [u8],
        stride: usize,
        band_rows: usize,
        erased: &'a Erased<'a, C>,
    ) -> Self {
        let shape = Shape::of(rows.len(), stride, band_rows);
        Self {
            base: rows.as_mut_ptr(),
            len: rows.len(),
            // `band_rows <= rows` (Shape), so this is at most `len + stride - 1`; saturating
            // only spells out that no wrap can make two bands overlap.
            band_bytes: shape.band_rows.saturating_mul(stride),
            band_rows: shape.band_rows,
            bands: shape.bands,
            cursor: AtomicUsize::new(0),
            erased: core::ptr::from_ref(erased).cast::<()>(),
            paint: call::<C>,
            _rows: PhantomData,
        }
    }

    /// How many bands the job holds.
    pub(crate) fn bands(&self) -> usize {
        self.bands
    }

    /// The next band, or `None` once every band was handed out.
    pub(crate) fn claim(&self) -> Option<Claim> {
        // Relaxed: the index's uniqueness is the read-modify-write's; the pixels are published
        // to the caller by the join, not by the cursor. Each painter stops at its first index
        // past `bands`, so the cursor grows by at most `bands + painters`: it cannot wrap.
        let i = self.cursor.fetch_add(1, Ordering::Relaxed);
        (i < self.bands).then_some(Claim(i))
    }

    /// Paints the band `claim` names.
    pub(crate) fn paint(&self, claim: Claim) {
        let i = claim.0;
        // `i < bands = ceil(rows / band_rows)` ⇒ `i * band_rows <= rows - 1` ⇒
        // `start <= (rows - 1) * stride < len`.
        let start = i * self.band_bytes;
        let end = start.saturating_add(self.band_bytes).min(self.len);
        // SAFETY: `[start, end)` lies inside the caller's rows (`end <= len`, `start < end` by the
        // line above), borrowed exclusively for `'a`, which the job does not outlive. Band `i`'s
        // range meets no other band's, and index `i` was handed out once (`claim`) and its claim
        // is consumed here — so this `&mut` aliases nothing for as long as it lives.
        let band = unsafe { core::slice::from_raw_parts_mut(self.base.add(start), end - start) };
        // SAFETY: `erased` and `paint` were made together by `Job::new` for one `C`, and the
        // `Erased<C>` outlives the job.
        unsafe { (self.paint)(self.erased, i * self.band_rows, band) };
    }

    /// Takes bands until none is left; returns how many this painter painted.
    pub(crate) fn take(&self) -> usize {
        let mut n = 0;
        while let Some(claim) = self.claim() {
            self.paint(claim);
            n += 1;
        }
        n
    }
}

/// `true` when every worker's stack canary survived the job.
pub(crate) struct Intact(pub(crate) bool);

/// Who paints beside the caller.
pub(crate) trait Crew {
    /// Publishes `job` to the workers; returns how many may take bands. `Err`: nothing was
    /// published — no worker will ever touch `job`.
    fn start(&self, job: &Job<'_>) -> Result<usize, PoolError>;

    /// Returns only once every worker [`Crew::start`] woke is done with the job — it will not
    /// touch it again — with whether their stacks survived. No deadline: the bands are finite.
    fn join(&self) -> Intact;
}

/// A started crew, joined when dropped — on an unwind through the caller's own bands too, so
/// the job never leaves its frame while a worker may still read it.
struct Pending<'w, W: Crew> {
    crew: &'w W,
    joined: bool,
}

impl<W: Crew> Pending<'_, W> {
    fn join(mut self) -> Intact {
        self.joined = true;
        self.crew.join()
    }
}

impl<W: Crew> Drop for Pending<'_, W> {
    fn drop(&mut self) {
        if !self.joined {
            let _ = self.crew.join();
        }
    }
}

/// The caller's side of a job: start the crew, take bands with it, join it. Returns how many
/// painters could take bands (0: no rows).
pub(crate) fn drive<W: Crew>(job: &Job<'_>, crew: &W) -> Result<usize, PoolError> {
    match job.bands() {
        0 => return Ok(0),
        // One band: nothing to share, no worker woken.
        1 => {
            job.take();
            return Ok(1);
        }
        _ => {}
    }
    let Ok(workers) = crew.start(job) else {
        // Nothing was published and no band is taken yet: the caller takes every band — the
        // same loop, so a band can never be painted twice.
        job.take();
        return Ok(1);
    };
    let pending = Pending { crew, joined: false };
    job.take();
    if pending.join().0 {
        Ok(workers + 1)
    } else {
        // The bands are complete, but a worker ran through its stack's floor: the process's
        // memory is suspect. Never a repaint — the caller treats it as fatal.
        Err(PoolError::StackOverflow)
    }
}

/// Paints `rows` (rows `stride` bytes apart; the last may be shorter) in bands of at least
/// `band_rows` rows on every hart the pool has, the caller among the painters: `f(ctx,
/// first_row, band)` for each band, `band` holding rows `first_row..` one after another.
/// Returns once every band is painted, with how many painters could take bands (1: the caller
/// alone — the pool not started, busy or poisoned, or one band; 0: no rows).
/// `Err(StackOverflow)`: every band is painted, but a worker overflowed its stack — fatal.
pub fn paint_bands<C: Sync>(
    rows: &mut [u8],
    stride: usize,
    band_rows: usize,
    ctx: &C,
    f: fn(&C, usize, &mut [u8]),
) -> Result<usize, PoolError> {
    let erased = Erased { ctx, f };
    let job = Job::new(rows, stride, band_rows, &erased);
    drive(&job, &here::crew())
}

/// The pool's workers as a crew.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod here {
    use core::cell::Cell;

    use super::{Crew, Intact, Job};
    use crate::pool::{self, PoolError};

    pub(super) struct Pool {
        /// The job's sequence number (set by `start`, read by `join`; the caller's thread only).
        seq: Cell<u64>,
    }

    pub(super) fn crew() -> Pool {
        Pool { seq: Cell::new(0) }
    }

    impl Crew for Pool {
        fn start(&self, job: &Job<'_>) -> Result<usize, PoolError> {
            let ctx = core::ptr::from_ref(job).cast_mut().cast::<u8>();
            let (workers, seq) = pool::submit(0, band_worker, ctx)?;
            self.seq.set(seq);
            Ok(workers)
        }

        fn join(&self) -> Intact {
            Intact(pool::join_scoped(self.seq.get()))
        }
    }

    /// A worker's side of a band job: take bands until none is left.
    extern "C" fn band_worker(_start: usize, _end: usize, ctx: *mut u8) {
        // SAFETY: `ctx` is the `&Job` `Pool::start` published for this job; its caller does not
        // return — the job stays where it is — until this worker has checked in after this call
        // returned (`pool::join_scoped`). `Job` is `Sync`.
        let job = unsafe { &*ctx.cast_const().cast::<Job<'_>>() };
        job.take();
    }
}

/// A build without the pool's threads: the caller paints every band.
#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
mod here {
    use super::{Crew, Intact, Job};
    use crate::pool::PoolError;

    pub(super) struct Inline;

    pub(super) fn crew() -> Inline {
        Inline
    }

    impl Crew for Inline {
        fn start(&self, _job: &Job<'_>) -> Result<usize, PoolError> {
            Err(PoolError::NotReady)
        }

        fn join(&self) -> Intact {
            Intact(true)
        }
    }
}

#[cfg(test)]
#[path = "bands_tests.rs"]
mod tests;
