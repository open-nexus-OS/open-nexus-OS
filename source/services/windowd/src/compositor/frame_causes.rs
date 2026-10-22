// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: why windowd sent a frame — the count of each cause over the loop window, printed
//! on the `windowd: loop hz=…` line. A present is one of four kinds at the point it leaves
//! (content was recomposited, a retained region was only re-blitted, only the pointer's region
//! moved, or nothing was named — a frame owed for what moved alone: a window's transform, a
//! body's scroll, the frame clock, TASK-0338); two causes are counted where they arise: the
//! frame clock's keep-alive and a pointer SHAPE change — which on a display with a hardware
//! pointer layer changes no pixel of the frame.
//!
//! WHY THIS FILE EXISTS: on the board every present is a full CPU composite into the scanned
//! plane (Block 4 layer 0, 2026-10-10), so "which presents need not exist" is a number the
//! scanout contract's first package is cut from — not a guess from reading the code.
//!
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU and board transcripts (`windowd: loop hz=… frames(…)`)
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

static CONTENT: AtomicU32 = AtomicU32::new(0);
static BLIT: AtomicU32 = AtomicU32::new(0);
static POINTER: AtomicU32 = AtomicU32::new(0);
static MOVED: AtomicU32 = AtomicU32::new(0);
static KEEPALIVE: AtomicU32 = AtomicU32::new(0);
static SHAPE: AtomicU32 = AtomicU32::new(0);
/// TASK-0335 P2: the damage queued (rectangles, their pixels) against what left for gpud
/// (the merged rectangles' pixels) — a bounding box that swallows the screen between two
/// small changes shows here as `flushed_px` far above `queued_px`.
static QUEUED_RECTS: AtomicU32 = AtomicU32::new(0);
static QUEUED_PX: AtomicU64 = AtomicU64::new(0);
static FLUSHED_PX: AtomicU64 = AtomicU64::new(0);
/// TASK-0339: windowd's own CPU work per frame — the longest flush, the app-surface rows copied
/// into the atlas (two syscalls a row) and the wallpaper rows rewritten into the retained plane
/// (a syscall a row), each with its microseconds. The measurement that splits an input's wait
/// into "windowd works" and "windowd waits".
static FLUSH_MAX_US: AtomicU64 = AtomicU64::new(0);
static COPY_ROWS: AtomicU64 = AtomicU64::new(0);
static COPY_US: AtomicU64 = AtomicU64::new(0);
static BASE_ROWS: AtomicU64 = AtomicU64::new(0);
static BASE_US: AtomicU64 = AtomicU64::new(0);
/// RFC-0109: the surface structures and motions refused — each is said once per surface, the
/// count of all of them rides the loop line (a refused motion drops its offsets too).
static LAYERS_REFUSED: AtomicU32 = AtomicU32::new(0);
/// One hop's window of the frame chain in windowd (RFC-0108 §hops, `nexus_ux_budgets::Hop`'s
/// shape in the per-second counters).
struct AtomicHop {
    n: AtomicU32,
    sum_us: AtomicU64,
    max_us: AtomicU64,
}

impl AtomicHop {
    const fn new() -> Self {
        Self { n: AtomicU32::new(0), sum_us: AtomicU64::new(0), max_us: AtomicU64::new(0) }
    }

    fn note(&self, us: u64) {
        self.n.fetch_add(1, Ordering::Relaxed);
        self.sum_us.fetch_add(us, Ordering::Relaxed);
        self.max_us.fetch_max(us, Ordering::Relaxed);
    }

    /// `(mean_us, max_us, count)` — and a fresh window.
    fn take(&self) -> (u64, u64, u32) {
        let n = self.n.swap(0, Ordering::Relaxed);
        let sum = self.sum_us.swap(0, Ordering::Relaxed);
        (sum / u64::from(n.max(1)), self.max_us.swap(0, Ordering::Relaxed), n)
    }
}

/// The frame chain's hops in windowd: a frame owed (the first change since the last submit) →
/// sent to gpud, and sent → its credit back (gpud's work, its flip, the blank it waits for).
static HOP_OWED_SENT: AtomicHop = AtomicHop::new();
static HOP_SENT_ACKED: AtomicHop = AtomicHop::new();
/// A present's issue time by its seq (two in flight: a ring of four holds them).
const ISSUE_RING: usize = 4;
#[allow(clippy::declare_interior_mutable_const)]
const ZERO_NS: AtomicU64 = AtomicU64::new(0);
static ISSUED_NS: [AtomicU64; ISSUE_RING] = [ZERO_NS; ISSUE_RING];

/// One frame left for gpud: `content` rects were recomposited, or a retained region was only
/// re-blitted (`blit`), or the pointer's region alone (`pointer`), or none of them — owed for
/// what moved alone.
pub(super) fn sent(content: bool, blit: bool, pointer: bool) {
    let kind = if content {
        &CONTENT
    } else if blit {
        &BLIT
    } else if pointer {
        &POINTER
    } else {
        &MOVED
    };
    kind.fetch_add(1, Ordering::Relaxed);
}

/// The frame clock owed a frame to keep its completions coming.
pub(super) fn keepalive() {
    KEEPALIVE.fetch_add(1, Ordering::Relaxed);
}

/// A pointer shape change queued a pointer-region present.
pub(super) fn shape() {
    SHAPE.fetch_add(1, Ordering::Relaxed);
}

/// A damage rectangle of `px` pixels was queued.
pub(super) fn queued(px: u64) {
    QUEUED_RECTS.fetch_add(1, Ordering::Relaxed);
    QUEUED_PX.fetch_add(px, Ordering::Relaxed);
}

/// A frame's merged content rectangles of `px` pixels left for gpud.
pub(super) fn flushed(px: u64) {
    FLUSHED_PX.fetch_add(px, Ordering::Relaxed);
}

/// One flush took `us`.
pub(super) fn work_flush(us: u64) {
    FLUSH_MAX_US.fetch_max(us, Ordering::Relaxed);
}

/// `rows` app-surface rows were copied into the atlas in `us`.
pub(super) fn work_copy(rows: u64, us: u64) {
    COPY_ROWS.fetch_add(rows, Ordering::Relaxed);
    COPY_US.fetch_add(us, Ordering::Relaxed);
}

/// `rows` wallpaper rows were rewritten into the retained plane in `us`.
pub(super) fn work_base(rows: u64, us: u64) {
    BASE_ROWS.fetch_add(rows, Ordering::Relaxed);
    BASE_US.fetch_add(us, Ordering::Relaxed);
}

/// Present `seq` left for gpud at `now_ns`.
pub(super) fn present_issued(seq: u32, now_ns: u64) {
    ISSUED_NS[seq as usize % ISSUE_RING].store(now_ns, Ordering::Relaxed);
}

/// Present `seq`'s credit came back at `now_ns`.
pub(super) fn present_acked(seq: u32, now_ns: u64) {
    let issued = ISSUED_NS[seq as usize % ISSUE_RING].load(Ordering::Relaxed);
    if issued != 0 && now_ns >= issued {
        HOP_SENT_ACKED.note((now_ns - issued) / 1000);
    }
}

/// A frame owed for `us` was sent.
pub(super) fn frame_sent_after(us: u64) {
    HOP_OWED_SENT.note(us);
}

/// The window's frame hops `[owed>sent, sent>acked]` as `(mean_us, max_us, count)`, and a fresh
/// window.
pub(super) fn take_frame_hops() -> [(u64, u64, u32); 2] {
    [HOP_OWED_SENT.take(), HOP_SENT_ACKED.take()]
}

/// A client's structure or motion frame was refused.
pub(super) fn layers_refused() {
    LAYERS_REFUSED.fetch_add(1, Ordering::Relaxed);
}

/// The refused structure and motion frames of the window, and a fresh count.
pub(super) fn take_refused() -> u32 {
    LAYERS_REFUSED.swap(0, Ordering::Relaxed)
}

/// The window's work `(flush_max_us, copy_rows, copy_us, base_rows, base_us)`, and a fresh one.
pub(super) fn take_work() -> (u64, u64, u64, u64, u64) {
    (
        FLUSH_MAX_US.swap(0, Ordering::Relaxed),
        COPY_ROWS.swap(0, Ordering::Relaxed),
        COPY_US.swap(0, Ordering::Relaxed),
        BASE_ROWS.swap(0, Ordering::Relaxed),
        BASE_US.swap(0, Ordering::Relaxed),
    )
}

/// The window's damage counts `(rects, queued_px, flushed_px)`, and a fresh window.
pub(super) fn take_damage() -> (u32, u64, u64) {
    (
        QUEUED_RECTS.swap(0, Ordering::Relaxed),
        QUEUED_PX.swap(0, Ordering::Relaxed),
        FLUSHED_PX.swap(0, Ordering::Relaxed),
    )
}

/// The window's counts `(content, blit, pointer, moved, keepalive, shape)`, and a fresh window.
pub(super) fn take() -> (u32, u32, u32, u32, u32, u32) {
    (
        CONTENT.swap(0, Ordering::Relaxed),
        BLIT.swap(0, Ordering::Relaxed),
        POINTER.swap(0, Ordering::Relaxed),
        MOVED.swap(0, Ordering::Relaxed),
        KEEPALIVE.swap(0, Ordering::Relaxed),
        SHAPE.swap(0, Ordering::Relaxed),
    )
}
