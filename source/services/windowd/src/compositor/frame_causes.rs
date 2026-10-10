// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: why windowd sent a frame — the count of each cause over the loop window, printed
//! on the `windowd: loop hz=…` line. A present is one of three kinds at the point it leaves
//! (content was recomposited, a retained region was only re-blitted, or only the pointer's
//! region moved); two causes that queue a pointer-region present without the pointer moving
//! are counted where they arise: the frame clock's keep-alive and a pointer SHAPE change —
//! which on a display with a hardware pointer layer changes no pixel of the frame.
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

use core::sync::atomic::{AtomicU32, Ordering};

static CONTENT: AtomicU32 = AtomicU32::new(0);
static BLIT: AtomicU32 = AtomicU32::new(0);
static POINTER: AtomicU32 = AtomicU32::new(0);
static KEEPALIVE: AtomicU32 = AtomicU32::new(0);
static SHAPE: AtomicU32 = AtomicU32::new(0);

/// One frame left for gpud: `content` rects were recomposited, or a retained region was only
/// re-blitted (`blit`), or neither — the pointer's region alone.
pub(super) fn sent(content: bool, blit: bool) {
    let kind = match (content, blit) {
        (true, _) => &CONTENT,
        (false, true) => &BLIT,
        (false, false) => &POINTER,
    };
    kind.fetch_add(1, Ordering::Relaxed);
}

/// The frame clock queued a pointer-region present to keep its completions coming.
pub(super) fn keepalive() {
    KEEPALIVE.fetch_add(1, Ordering::Relaxed);
}

/// A pointer shape change queued a pointer-region present.
pub(super) fn shape() {
    SHAPE.fetch_add(1, Ordering::Relaxed);
}

/// The window's counts `(content, blit, pointer, keepalive, shape)`, and a fresh window.
pub(super) fn take() -> (u32, u32, u32, u32, u32) {
    (
        CONTENT.swap(0, Ordering::Relaxed),
        BLIT.swap(0, Ordering::Relaxed),
        POINTER.swap(0, Ordering::Relaxed),
        KEEPALIVE.swap(0, Ordering::Relaxed),
        SHAPE.swap(0, Ordering::Relaxed),
    )
}
