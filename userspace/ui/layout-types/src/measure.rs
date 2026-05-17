// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::node::TextContent;
use crate::text::TextStyle;
use crate::types::FxPx;
use alloc::vec::Vec;
use core::ops::Range;

// ─── Prepared text handle ───

/// Opaque handle to prepared (shaped, bidi-resolved) text.
/// Backed by the paragraph/run cache in nexus-shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreparedTextHandle(pub usize);

// ─── Line layout ───

/// Result of line-breaking prepared text for a specific width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineLayout {
    pub lines: Vec<LineMetrics>,
    pub natural_width: FxPx,
}

/// Metrics for a single wrapped line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMetrics {
    /// Character range into the original text.
    pub text_range: Range<usize>,
    /// Measured width of this line.
    pub width: FxPx,
    /// Baseline offset from line top.
    pub baseline: FxPx,
    /// Total line height (ascent + descent + line gap).
    pub height: FxPx,
}

// ─── Measurement callback trait ───

/// The layout engine calls this trait to measure text.
///
/// Implementations live in the shaping crate (`nexus-shape`) or a host test
/// harness. This trait is defined here (in `nexus-layout-types`) so the
/// layout engine crate (`nexus-layout`) can call it without depending on
/// the shaping backend.
pub trait MeasureText {
    /// Prepare text for measurement (shaping, bidi).
    /// Returns an opaque handle valid for the lifetime of the layout pass.
    fn prepare(&self, content: &TextContent, style: &TextStyle) -> PreparedTextHandle;

    /// Measure the natural advance width of prepared text (no wrapping).
    fn measure_width(&self, handle: &PreparedTextHandle) -> FxPx;

    /// Layout prepared text into lines for a given width.
    ///
    /// `max_lines`: if set, truncate after this many lines.
    fn layout_lines(
        &self,
        handle: &PreparedTextHandle,
        width: FxPx,
        max_lines: Option<u32>,
    ) -> LineLayout;
}
