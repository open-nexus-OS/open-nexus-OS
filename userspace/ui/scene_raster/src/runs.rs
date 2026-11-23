// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the painter's per-pixel loops laid as runs through the paint kernels (TASK-0367 P4):
//! a loop whose every pixel takes one colour at its own alpha — a shadow's falloff, a circle's
//! and a polygon's anti-aliased pixels — asks its alphas first, into a fixed stack row, and the
//! row goes through `nexus_paint_kernels::cover`; a sprite laid 1:1 goes through
//! `nexus_paint_kernels::over`. The bytes are those of `RowCanvas::blend` on each pixel in turn:
//! an alpha depends on the geometry alone, never on the row, and the run is the `visible` part of
//! the span, where every pixel lands.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `span_identity` (a run against `blend` pixel by pixel, scrolled, scissored and
//! under a rounded clip); the scene goldens.

use super::RowCanvas;
use crate::shapes::covered;
use nexus_layout_types::Rgba8;

/// Pixels whose alphas a run asks before it lays them: a fixed stack row, no allocation.
const RUN: usize = 128;

/// The alpha `RowCanvas::blend_covered` lays `c` with at `coverage`: `c`'s own whole, rounded
/// down to the covered share in between, nothing at none.
#[inline]
pub(crate) fn covered_alpha(c: Rgba8, coverage: f32) -> u8 {
    if coverage >= 1.0 {
        c.a
    } else if coverage > 0.0 {
        covered(c, coverage).a
    } else {
        0
    }
}

impl RowCanvas<'_> {
    /// `blend(x, Rgba8 { a: alpha(x), ..c })` for every pixel of model span `[x0, x1)` that
    /// lands, in one kernel call a stack row of pixels (an alpha of 0 leaves its pixel, as
    /// `blend` does). `alpha` is asked once per landing pixel, in order.
    pub(crate) fn cover_run(
        &mut self,
        x0: i32,
        x1: i32,
        c: Rgba8,
        mut alpha: impl FnMut(i32) -> u8,
    ) {
        let (x0, x1) = self.visible(x0, x1);
        let mut row = [0u8; RUN];
        let mut x = x0;
        while x < x1 {
            let n = ((x1 - x) as usize).min(RUN);
            for (i, a) in row[..n].iter_mut().enumerate() {
                let xx = x + i as i32;
                *a = alpha(xx);
                // A rounded clip's soft column lets through what `blend` lets through.
                if let Some(k) = self.round.as_ref().filter(|_| *a != 0) {
                    *a = k.cover(xx - self.shift_x, Rgba8 { a: *a, ..c }).a;
                }
            }
            // `visible` keeps `x - shift_x` inside the row and the scissor: it is the pixel.
            let b0 = (x - self.shift_x) as usize * 4;
            if let Some(dst) = self.buf.get_mut(b0..b0 + 4 * n) {
                nexus_paint_kernels::cover(dst, [c.b, c.g, c.r], &row[..n]);
            }
            x += n as i32;
        }
    }

    /// `blend` of each straight BGRA pixel of `src` from model-x `x0` on, every one of them
    /// landing — the caller hands the `visible` span and no rounded clip is set. `false` (and
    /// nothing painted) when the span does not land whole: the caller paints pixel by pixel.
    pub(crate) fn over_run(&mut self, x0: i32, src: &[u8]) -> bool {
        let n = (src.len() / 4) as i32;
        if self.round.is_some() || self.visible(x0, x0 + n) != (x0, x0 + n) {
            return false;
        }
        let b0 = (x0 - self.shift_x) as usize * 4;
        let Some(dst) = self.buf.get_mut(b0..b0 + 4 * n as usize) else { return false };
        nexus_paint_kernels::over(dst, src);
        true
    }
}
