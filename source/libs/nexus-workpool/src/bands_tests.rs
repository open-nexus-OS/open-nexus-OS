// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The band entry's host model (TASK-0367 P2), run under miri as well: the real `Job` (cursor,
//! claims, band slices, the erased painter) and the real `drive`, with crews that inject the
//! interleavings — claims a worker makes before the caller starts and paints only in the join,
//! start refusals of every kind, a dead canary — and a crew of real threads.

use super::*;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Row stride of the model's rows (bytes).
const STRIDE: usize = 12;

/// The byte every painter must leave at offset `o` of the rows.
fn golden(o: usize) -> u8 {
    let (r, c) = (o / STRIDE, o % STRIDE);
    (r.wrapping_mul(31) ^ c.wrapping_mul(7) ^ 0x5A) as u8
}

/// What a pass's painter sees and leaves: how often each row was painted.
struct Rec {
    row_hits: Vec<AtomicUsize>,
    /// Yield between rows (real threads: more interleavings).
    yields: bool,
}

impl Rec {
    fn new(len: usize, yields: bool) -> Self {
        Self { row_hits: (0..len.div_ceil(STRIDE)).map(|_| AtomicUsize::new(0)).collect(), yields }
    }

    /// Every row painted exactly once.
    fn once(&self) -> bool {
        self.row_hits.iter().all(|h| h.load(Ordering::Acquire) == 1)
    }
}

fn paint(rec: &Rec, first_row: usize, band: &mut [u8]) {
    for (r, row) in band.chunks_mut(STRIDE).enumerate() {
        for (c, b) in row.iter_mut().enumerate() {
            *b = golden((first_row + r) * STRIDE + c);
        }
        rec.row_hits[first_row + r].fetch_add(1, Ordering::AcqRel);
        if rec.yields {
            std::thread::yield_now();
        }
    }
}

fn is_golden(rows: &[u8]) -> bool {
    rows.iter().enumerate().all(|(o, &b)| b == golden(o))
}

/// A crew whose workers run where the script says, on the caller's thread.
struct Scripted<'j> {
    job: &'j Job<'j>,
    workers: usize,
    /// `start` refuses with this (nothing published).
    refuse: Option<PoolError>,
    /// Bands each worker claims in `start`, before the caller takes any.
    early: usize,
    /// The early claims are painted only in the join (a worker mid-band when the caller is done).
    defer: bool,
    /// The join paints the claims it holds (`false`: a join that returns early — the broken
    /// entry the oracle must catch).
    finish: bool,
    intact: bool,
    held: RefCell<Vec<Claim>>,
    started: Cell<bool>,
    worker_bands: Cell<usize>,
}

impl<'j> Scripted<'j> {
    fn new(job: &'j Job<'j>, workers: usize) -> Self {
        Self {
            job,
            workers,
            refuse: None,
            early: 0,
            defer: false,
            finish: true,
            intact: true,
            held: RefCell::new(Vec::new()),
            started: Cell::new(false),
            worker_bands: Cell::new(0),
        }
    }
}

impl Crew for Scripted<'_> {
    fn start(&self, job: &Job<'_>) -> Result<usize, PoolError> {
        assert!(core::ptr::eq(core::ptr::from_ref(job).cast::<()>(), {
            core::ptr::from_ref(self.job).cast::<()>()
        }));
        if let Some(e) = self.refuse {
            return Err(e);
        }
        self.started.set(true);
        for _ in 0..self.workers {
            for _ in 0..self.early {
                let Some(claim) = job.claim() else { break };
                if self.defer {
                    self.held.borrow_mut().push(claim);
                } else {
                    job.paint(claim);
                    self.worker_bands.set(self.worker_bands.get() + 1);
                }
            }
        }
        Ok(self.workers)
    }

    fn join(&self) -> Intact {
        if self.finish {
            for claim in self.held.borrow_mut().drain(..).rev() {
                self.job.paint(claim);
                self.worker_bands.set(self.worker_bands.get() + 1);
            }
            // The workers wake late and find what the caller left: nothing.
            for _ in 0..self.workers {
                self.worker_bands.set(self.worker_bands.get() + self.job.take());
            }
        }
        Intact(self.intact)
    }
}

/// A crew of real threads: each worker takes bands and checks in; the join waits for every
/// check-in (the model of `pool::join_scoped`'s count).
struct Threads<'s, 'e> {
    scope: &'s std::thread::Scope<'s, 'e>,
    job: &'e Job<'e>,
    workers: usize,
    done: &'e AtomicUsize,
}

impl Crew for Threads<'_, '_> {
    fn start(&self, _job: &Job<'_>) -> Result<usize, PoolError> {
        for _ in 0..self.workers {
            let (job, done) = (self.job, self.done);
            self.scope.spawn(move || {
                job.take();
                done.fetch_add(1, Ordering::AcqRel);
            });
        }
        Ok(self.workers)
    }

    fn join(&self) -> Intact {
        while self.done.load(Ordering::Acquire) < self.workers {
            std::thread::yield_now();
        }
        Intact(true)
    }
}

/// The pass lengths the model cuts: one row, short passes, a partial last row, tall passes.
const LENS: [usize; 6] = [STRIDE, 7 * STRIDE, 8 * STRIDE + 5, 9 * STRIDE, 33 * STRIDE, 70 * STRIDE];

fn retryable(r: Result<usize, PoolError>) -> bool {
    matches!(
        r,
        Err(PoolError::Busy
            | PoolError::Poisoned
            | PoolError::NotReady
            | PoolError::Abi
            | PoolError::Timeout)
    )
}

#[test]
fn bands_shape_holds_its_bounds() {
    for len in 1..600usize {
        for stride in [1usize, 3, 12, 64] {
            for ask in [0usize, 1, 7, 8, 9, 40, 1000] {
                let s = Shape::of(len, stride, ask);
                let rows = len.div_ceil(stride);
                assert!(s.bands >= 1 && s.bands <= MAX_BANDS, "{len} {stride} {ask}: {s:?}");
                assert!(s.band_rows >= MIN_BAND_ROWS.min(rows), "{len} {stride} {ask}: {s:?}");
                assert!(s.band_rows <= rows.max(ask), "{len} {stride} {ask}: {s:?}");
                assert!(s.bands * s.band_rows >= rows && (s.bands - 1) * s.band_rows < rows);
            }
        }
    }
    assert_eq!(Shape::of(0, 12, 8).bands, 0);
    assert_eq!(Shape::of(12, 0, 8).bands, 0);
    // A taller pass takes taller bands.
    assert_eq!(Shape::of(2000 * 4, 4, 8), Shape { band_rows: 16, bands: 125 });
}

/// The cursor hands each band out once — to concurrent claimers too — and refuses every claim
/// past the last; driven by 1..8 painters in every scripted order, each row is painted once.
#[test]
fn test_reject_a_band_taken_twice() {
    let rec = Rec::new(70 * STRIDE, false);
    let mut rows = vec![0u8; 70 * STRIDE];
    let erased = Erased { ctx: &rec, f: paint };
    let job = Job::new(&mut rows, STRIDE, 8, &erased);
    let bands = job.bands();
    let mut seen: Vec<usize> = std::thread::scope(|s| {
        let claimers: Vec<_> = (0..4)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    while let Some(c) = job.claim() {
                        mine.push(c.0);
                    }
                    mine
                })
            })
            .collect();
        claimers.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });
    seen.sort_unstable();
    assert_eq!(seen, (0..bands).collect::<Vec<_>>(), "every band handed out exactly once");
    assert!((0..16).all(|_| job.claim().is_none()), "a spent cursor refuses every claim");

    for len in LENS {
        for ask in [1usize, 8, 13] {
            for workers in 0..8 {
                for early in [0usize, 1, 2, 5] {
                    for defer in [false, true] {
                        let rec = Rec::new(len, false);
                        let mut rows: Vec<u8> = (0..len).map(|i| (i * 13 + 1) as u8).collect();
                        let erased = Erased { ctx: &rec, f: paint };
                        let job = Job::new(&mut rows, STRIDE, ask, &erased);
                        let crew = Scripted { early, defer, ..Scripted::new(&job, workers) };
                        let got = drive(&job, &crew);
                        let at = format!("len={len} ask={ask} workers={workers} early={early}");
                        assert!(got.is_ok(), "{at}: {got:?}");
                        assert!(rec.once(), "{at}: a row painted twice or skipped");
                        assert!(is_golden(&rows), "{at}: bytes");
                    }
                }
            }
        }
    }
}

/// A worker that claimed bands before the caller started and paints them only in the join:
/// when the call returns they are painted. A join that returns early leaves rows unpainted —
/// the oracle sees it (so the check above is not vacuous).
#[test]
fn test_reject_returning_before_every_taken_band_is_done() {
    for len in LENS {
        for workers in 1..8 {
            let rec = Rec::new(len, false);
            let mut rows = vec![0u8; len];
            let erased = Erased { ctx: &rec, f: paint };
            let job = Job::new(&mut rows, STRIDE, 8, &erased);
            let crew = Scripted { early: 1, defer: true, ..Scripted::new(&job, workers) };
            assert!(drive(&job, &crew).is_ok());
            assert!(rec.once(), "len={len} workers={workers}: returned before a held band");
            assert!(is_golden(&rows));
        }
    }
    let rec = Rec::new(70 * STRIDE, false);
    let mut rows = vec![0u8; 70 * STRIDE];
    let erased = Erased { ctx: &rec, f: paint };
    let job = Job::new(&mut rows, STRIDE, 8, &erased);
    let broken = Scripted { early: 2, defer: true, finish: false, ..Scripted::new(&job, 3) };
    assert!(drive(&job, &broken).is_ok());
    assert!(!rec.once(), "a join that returns early leaves held bands unpainted");

    // Real threads that yield between rows: at the return every row is painted.
    for workers in 1..8 {
        let len = 33 * STRIDE;
        let rec = Rec::new(len, true);
        let mut rows = vec![0u8; len];
        let erased = Erased { ctx: &rec, f: paint };
        let job = Job::new(&mut rows, STRIDE, 1, &erased);
        let done = AtomicUsize::new(0);
        let got = std::thread::scope(|scope| {
            let crew = Threads { scope, job: &job, workers, done: &done };
            let got = drive(&job, &crew);
            // The join counted every worker in before `drive` returned.
            assert_eq!(done.load(Ordering::Acquire), workers);
            got
        });
        assert_eq!(got, Ok(workers + 1));
        assert!(rec.once(), "{workers} workers");
        assert!(is_golden(&rows));
    }
}

/// A start refused for any reason publishes nothing: the caller paints every band and the call
/// is `Ok(1)`. After a start the outcome is `Ok(painters)` or — a dead canary, the bands complete
/// and never repainted — `Err(StackOverflow)`; never an error that invites painting again.
#[test]
fn test_reject_a_retryable_err_after_a_band_was_taken() {
    let refusals = [
        PoolError::Busy,
        PoolError::Poisoned,
        PoolError::NotReady,
        PoolError::Abi,
        PoolError::Timeout,
        PoolError::AlreadyInitialized,
    ];
    for len in LENS {
        let bands = Shape::of(len, STRIDE, 8).bands;
        for refuse in refusals {
            let rec = Rec::new(len, false);
            let mut rows = vec![0u8; len];
            let erased = Erased { ctx: &rec, f: paint };
            let job = Job::new(&mut rows, STRIDE, 8, &erased);
            let crew = Scripted { refuse: Some(refuse), early: 3, ..Scripted::new(&job, 4) };
            let got = drive(&job, &crew);
            assert_eq!(got, Ok(if bands == 0 { 0 } else { 1 }), "{refuse:?}");
            assert_eq!(crew.worker_bands.get(), 0, "{refuse:?}: a worker painted after a refusal");
            assert!(rec.once(), "{refuse:?}");
            assert!(is_golden(&rows));
        }
        for intact in [true, false] {
            for early in [0usize, 1, 3] {
                let rec = Rec::new(len, false);
                let mut rows = vec![0u8; len];
                let erased = Erased { ctx: &rec, f: paint };
                let job = Job::new(&mut rows, STRIDE, 8, &erased);
                let crew = Scripted { intact, early, defer: true, ..Scripted::new(&job, 3) };
                let got = drive(&job, &crew);
                assert!(!retryable(got), "len={len}: {got:?} after bands were taken");
                match (bands, intact) {
                    (0 | 1, _) => assert!(got.is_ok() && !crew.started.get()),
                    (_, true) => assert_eq!(got, Ok(4)),
                    (_, false) => assert_eq!(got, Err(PoolError::StackOverflow)),
                }
                assert!(rec.once(), "len={len} intact={intact}: never a repaint");
            }
        }
    }
}

/// The public entry on a build without the pool's threads: the caller paints every band.
#[test]
fn bands_paint_inline_without_the_pool() {
    let len = 70 * STRIDE + 3;
    let rec = Rec::new(len, false);
    let mut rows = vec![0u8; len];
    assert_eq!(paint_bands(&mut rows, STRIDE, 8, &rec, paint), Ok(1));
    assert!(rec.once() && is_golden(&rows));
    assert_eq!(paint_bands(&mut [], STRIDE, 8, &rec, paint), Ok(0));
}
