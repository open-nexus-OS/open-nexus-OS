// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The three passes painted in place by bands (TASK-0367 P2), against the scratch rows they
//! replaced — the base on its slot rows, a scroll band and a motion layer on their store columns
//! (the canvas shifted to the slice) — in a buffer whose rows are `STRIDE` apart and hold
//! garbage: for 1 to 8 painters on threads, every split, bands in either order, and through the
//! real `nexus_workpool::paint_bands`: every byte of the pass the same, every byte outside it
//! untouched. A row allocates nothing (a counting allocator on this thread).

use super::tests::{scene, GROUND, H, ROW_BYTES, W};
use super::*;
use crate::straight_alpha::premultiplied;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// Rows lie this far apart: past a row's pixels (RFC-0114 §1's stride), so a pass that writes
/// outside its bytes shows.
const STRIDE: usize = ROW_BYTES + 36;

/// A buffer of `rows` rows `STRIDE` apart (the last row ends with its pixels), holding garbage.
fn garbage(rows: usize) -> Vec<u8> {
    (0..(rows - 1) * STRIDE + ROW_BYTES).map(|i| (i * 131 + 7) as u8).collect()
}

/// What a pass leaves: `rows` (buffer of `garbage`) with each row's `[x, x + len)` replaced by
/// `row(y)` (a full scratch row's bytes `[from, from + len)`).
fn expected(
    rows: usize,
    (x, len, from): (usize, usize, usize),
    mut row: impl FnMut(i32, &mut [u8]),
) -> Vec<u8> {
    let mut out = garbage(rows);
    let mut scratch = vec![0u8; ROW_BYTES];
    for r in 0..rows {
        row(r as i32, &mut scratch);
        let at = r * STRIDE + x;
        out[at..at + len].copy_from_slice(&scratch[from..from + len]);
    }
    out
}

/// Paints `rows` by `pass` on `painters` threads, bands of `band_rows` taken from one queue
/// (front to back or back to front).
fn on_threads(pass: &BandPass<'_>, rows: &mut [u8], painters: usize, band_rows: usize, rev: bool) {
    let mut bands: Vec<(usize, &mut [u8])> =
        rows.chunks_mut(band_rows * STRIDE).enumerate().map(|(i, b)| (i * band_rows, b)).collect();
    if !rev {
        bands.reverse(); // popped from the back
    }
    let queue = std::sync::Mutex::new(bands);
    std::thread::scope(|s| {
        for _ in 0..painters {
            s.spawn(|| loop {
                let next = queue.lock().ok().and_then(|mut q| q.pop());
                let Some((first, band)) = next else { break };
                paint_band(pass, first, band);
            });
        }
    });
}

/// Every split and order for 1..8 painters, and the real entry: each equal to `want`.
fn every_way(pass: &BandPass<'_>, rows: usize, want: &[u8], what: &str) {
    for painters in 1..=8 {
        for band_rows in [1usize, 3, 8, 17, rows] {
            for rev in [false, true] {
                let mut got = garbage(rows);
                on_threads(pass, &mut got, painters, band_rows, rev);
                assert!(
                    got == want,
                    "{what}: {painters} painters, bands of {band_rows}, rev={rev}"
                );
            }
        }
    }
    let mut got = garbage(rows);
    assert!(pass.fits(&got));
    assert!(pass.paint(&mut got).is_ok());
    assert!(got == want, "{what}: through paint_bands");
}

/// The base: surface rows `[y0, y1)` in place over the page base, whole rows and a column window.
#[test]
fn base_bands_equal_the_scratch_rows_for_any_painters() {
    let s = scene();
    let plan = s.plan();
    let (y0, y1) = (40, H - 20);
    let rows = (y1 - y0) as usize;
    let want =
        expected(rows, (0, ROW_BYTES, 0), |r, row| plan.paint_ground_row(row, y0 + r, GROUND));
    let pass = BandPass::base(plan, (y0, STRIDE), ROW_BYTES, GROUND, None);
    every_way(&pass, rows, &want, "base");

    let cols = (90, 333);
    let want = expected(rows, (0, ROW_BYTES, 0), |r, row| {
        row.fill(0);
        plan.paint_ground_cols(row, y0 + r, GROUND, cols);
    });
    // The scratch row was zero outside the columns; the slot's row keeps its garbage there.
    let mut want = want;
    let base = garbage(rows);
    for r in 0..rows {
        let at = r * STRIDE;
        let (c0, c1) = (at + cols.0 as usize * 4, at + cols.1 as usize * 4);
        want[at..c0].copy_from_slice(&base[at..c0]);
        want[c1..at + ROW_BYTES].copy_from_slice(&base[c1..at + ROW_BYTES]);
    }
    let pass = BandPass::base(plan, (y0, STRIDE), ROW_BYTES, GROUND, Some(cols));
    every_way(&pass, rows, &want, "base cols");
}

/// A motion layer: its rest `rect` painted from transparent with the canvas at its left edge,
/// in place at store column `sx` — the scratch row's first `span` bytes before. The rest starts
/// right of the panel's shadow and holds the animated card, the icon, the halo and the field.
#[test]
fn motion_bands_equal_the_scratch_rows_for_any_painters() {
    let s = scene();
    let plan = s.plan();
    let rect = (300, 20, 470, 300);
    let span = (rect.2 - rect.0) as usize * 4;
    let sx = 37usize; // the store column: anywhere, the canvas moves with it
    let rows = (rect.3 - rect.1) as usize;
    let want = expected(rows, (sx * 4, span, 0), |r, row| {
        row.fill(0);
        plan.paint_row(row, rect.1 + r, rect.0);
        unpremultiply(&mut row[..span]);
    });
    let pass = BandPass::layer(plan, (rect.1, STRIDE), (sx * 4, span), None, rect.0);
    every_way(&pass, rows, &want, "motion");
    // The pass draws: its rows are not the transparent ground alone.
    assert!(
        want.chunks(STRIDE).filter(|r| r[sx * 4..sx * 4 + span].iter().any(|&b| b != 0)).count()
            > rows / 2
    );
}

/// A scroll band: its content at identity (no views), columns `[c0, c1)` of the row the painter
/// saw, in place at store column `col` — vertical (shift 0) and horizontal (a run of columns),
/// on its opaque ground and on transparent.
#[test]
fn scroll_bands_equal_the_scratch_rows_for_any_painters() {
    let s = scene();
    let plan = PassPlan { views: &[], ..s.plan() };
    let pane = nexus_layout_types::Rgba8::new(20, 24, 28, 255);
    for (shift, (c0, c1), col) in [(0, (24, 324), 100usize), (150, (0, 220), 5), (0, (0, W), 0)] {
        for ground in [None, Some(premultiplied(pane, pane.a))] {
            let (y0, rows) = (180, 150usize);
            let span = (c1 - c0) as usize * 4;
            let want = expected(rows, (col * 4, span, c0 as usize * 4), |r, row| {
                match ground {
                    Some(g) => nexus_paint_kernels::fill(row, g),
                    None => row.fill(0),
                }
                plan.paint_row(row, y0 + r, shift);
                unpremultiply(&mut row[c0 as usize * 4..c1 as usize * 4]);
            });
            let pass = BandPass::layer(plan, (y0, STRIDE), (col * 4, span), ground, shift + c0);
            every_way(&pass, rows, &want, &format!("band shift={shift} cols={c0}..{c1}"));
        }
    }
}

/// Rows that do not hold the pass's columns are refused before any pixel is painted.
#[test]
fn test_reject_rows_without_the_pass_columns() {
    let s = scene();
    let plan = s.plan();
    let rows = garbage(4);
    let pass = BandPass::layer(plan, (0, STRIDE), (ROW_BYTES - 8, 12), None, 0);
    assert!(!pass.fits(&rows), "the last row ends with its pixels");
    assert!(BandPass::layer(plan, (0, STRIDE), (ROW_BYTES - 8, 8), None, 0).fits(&rows));
    assert!(!BandPass::layer(plan, (0, STRIDE), (usize::MAX, 8), None, 0).fits(&rows));
    assert!(BandPass::base(plan, (0, STRIDE), ROW_BYTES, GROUND, None).fits(&[]));
}

thread_local! {
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

/// Counts this thread's allocations (each test runs on its own thread).
struct Counting;

// SAFETY: test-only (never in the OS image). Every method forwards to `System`, which upholds the
// `GlobalAlloc` contract; the only addition is a non-allocating thread-local counter bump.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        // SAFETY: the caller's `GlobalAlloc::alloc` contract, passed on unchanged.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System` through this allocator with this `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        // SAFETY: the caller's `GlobalAlloc::realloc` contract, passed on unchanged.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

fn allocs() -> usize {
    ALLOCS.with(Cell::get)
}

/// A band of every kind — the base whole and in columns, a motion layer, a scroll band on its
/// ground — paints its rows without one allocation: no worker takes the service heap's lock per
/// row. (Picks, glyph runs, the selection, the halo and the shadows all paint in these rows.)
#[test]
fn paint_row_allocates_nothing() {
    let s = scene();
    let plan = s.plan();
    let rows = H as usize;
    let mut buf = garbage(rows);
    let passes = [
        BandPass::base(plan, (0, STRIDE), ROW_BYTES, GROUND, None),
        BandPass::base(plan, (0, STRIDE), ROW_BYTES, GROUND, Some((60, 400))),
        BandPass::layer(plan, (0, STRIDE), (40 * 4, 170 * 4), None, 300),
        BandPass::layer(
            PassPlan { views: &[], ..plan },
            (0, STRIDE),
            (0, 300 * 4),
            Some([9; 4]),
            24,
        ),
    ];
    for pass in &passes {
        let before = allocs();
        paint_band(pass, 0, &mut buf);
        assert_eq!(allocs() - before, 0, "a band's rows allocated");
    }
    // The counter counts: an allocation on this thread shows.
    let before = allocs();
    let v = std::hint::black_box(vec![0u8; 64]);
    assert!(allocs() > before && v.len() == 64);
}
