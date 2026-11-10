// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(any(test, feature = "std")), no_std)]
// RFC-0080: the shared-atlas base is a raw mapped-VMO pointer → slice, which is
// inherently `unsafe`. `deny` (not `forbid`) so ONLY the two atlas-base items
// below may opt in with `#[allow(unsafe_code)]` + a documented safety contract;
// everything else stays unsafe-free.
#![deny(unsafe_code)]

//! CONTEXT: `nexus-text-baked` — the shared text SSOT: runtime measurement +
//! row-based A8 glyph rendering over the build-time-baked atlases of the
//! vendored UI face. PROMOTED VERBATIM from windowd's `text.rs`/`build.rs`
//! (RFC-0067 P5 discipline: promote the best implementation, make windowd a
//! client) so the app-host runtime, windowd, and future DSL shells measure
//! and draw text identically. `no_std`, zero deps, no runtime font parsing.
//! The faces are the design's text styles (RFC-0116): [`FontSize`] names
//! one baked `(size, weight)` face per style and weight override, and a
//! run's letter-spacing ([`Tracking`]) is placed by the same pen in
//! [`measure`], [`advance`], [`ellipsis_cut`] and [`draw_text_row`].
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `src/tests.rs` (drawing, fallback, atlas budget, frozen metrics, weights,
//! tracking, the display face), `src/glow.rs` (the blurred shadow), `src/ladder.rs`,
//! `src/metrics.rs`, `src/measure_text.rs`
//!
//! The API is ROW-BASED to match the surface renderers: every shell surface
//! is painted one pixel row at a time, so [`draw_text_row`] blends exactly
//! the slice of a text run that intersects the current row. A text run
//! occupies the band `top .. top + line_height(size)` with its baseline
//! `ascent(size)` pixels below the band top; rows outside the band return
//! immediately.

#[cfg(any(test, not(feature = "std"), feature = "layout"))]
extern crate alloc;

/// Pixel-real [`MeasureText`](nexus_layout_types::MeasureText) over the
/// baked atlases (feature `layout`).
#[cfg(feature = "layout")]
pub mod measure_text;

mod glow;
mod ladder;
mod metrics;
mod pen;
#[cfg(test)]
mod tests;
pub use glow::{blur_reach, draw_text_blur_row};
use ladder::{face, Face};
pub use ladder::{FontSize, Weight};
pub use metrics::{advance, ellipsis_cut, measure, ELLIPSIS};
use pen::Pen;
pub use pen::Tracking;

#[allow(clippy::all)]
mod baked {
    include!(concat!(env!("OUT_DIR"), "/baked_fonts.rs"));
}

use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// RFC-0080: the process-global base of the glyph-coverage atlas — the ONE
/// backing that every `Face.cov()` slices. Set once via [`set_atlas_base`]
/// (an app-host installs its shared RO VMO mapping here). Until then it is
/// null; with the `embedded-atlas` feature it lazily auto-inits to the linked
/// blob, so windowd/host/tests need no setup and stay byte-identical.
static ATLAS_PTR: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
static ATLAS_LEN: AtomicUsize = AtomicUsize::new(0);

/// Installs the atlas backing (RFC-0080): `ptr` must point at
/// [`atlas_len()`] readable bytes that stay valid for the process lifetime
/// (a mapped RO VMO). Idempotent; a consumer with `embedded-atlas` OFF MUST
/// call this before rendering any text.
///
/// # Safety
/// `ptr` must be valid and immutable for `len` bytes for the whole process
/// lifetime, and `len` must equal [`atlas_len()`] (the baked atlas size).
#[allow(unsafe_code)]
pub unsafe fn set_atlas_base(ptr: *const u8, len: usize) {
    ATLAS_LEN.store(len, Ordering::Release);
    ATLAS_PTR.store(ptr as *mut u8, Ordering::Release);
}

/// The baked atlas size in bytes (what [`set_atlas_base`] must be given).
#[must_use]
pub const fn atlas_len() -> usize {
    baked::ATLAS_LEN
}

/// The embedded atlas bytes (feature `embedded-atlas`) — the VMO owner's fill
/// source (RFC-0080 Phase 1): it `vmo_write`s these into a shared RO VMO that
/// every app-host maps via [`set_atlas_base`].
#[cfg(feature = "embedded-atlas")]
#[must_use]
pub fn embedded_atlas() -> &'static [u8] {
    baked::EMBEDDED_ATLAS
}

/// Resolves the whole coverage atlas as a slice. Falls back to the embedded
/// blob (feature `embedded-atlas`) when no base has been installed; returns an
/// empty slice otherwise (missing glyphs render blank — fail-visible, never
/// garbage).
#[inline]
#[allow(unsafe_code)]
fn atlas() -> &'static [u8] {
    let ptr = ATLAS_PTR.load(Ordering::Acquire);
    if ptr.is_null() {
        // No installed base: the lazy default is the linked blob (byte-
        // identical to the old path), or empty when it isn't linked.
        #[cfg(feature = "embedded-atlas")]
        {
            baked::EMBEDDED_ATLAS
        }
        #[cfg(not(feature = "embedded-atlas"))]
        {
            &[]
        }
    } else {
        let len = ATLAS_LEN.load(Ordering::Acquire);
        // SAFETY: the base was installed via `set_atlas_base`, whose contract
        // requires `ptr` valid + immutable for `len` bytes for the process
        // lifetime.
        unsafe { core::slice::from_raw_parts(ptr, len) }
    }
}

/// Height of a text band (ascent + descent, no extra leading — callers add
/// their own line spacing).
#[must_use]
pub const fn line_height(size: FontSize) -> u32 {
    face(size).line_h
}

/// Baseline offset from the band top.
#[must_use]
pub const fn ascent(size: FontSize) -> i32 {
    face(size).ascent
}

/// Average glyph advance — the wrap heuristic for interim char-count
/// wrapping (measured wrapping lands with the layout unification).
#[must_use]
pub const fn avg_advance(size: FontSize) -> u32 {
    face(size).avg_advance
}

/// Glyph index for a char WITHIN one face: dense ASCII by offset, the sparse
/// EXTRAS tail (umlauts/ß, ± ÷ × −), then the WIDE tail (kana/jamo/hangul/han,
/// RFC-0075 Phase 8d) by binary search. `None` when the codepoint is outside
/// this face's charset OR the slot is an absent glyph (a `Latin` face carries
/// ASCII-shaped slots for indices it does not rasterize, so the dense scheme
/// survives — an absent slot is `w == 0 && advance == 0`).
#[inline]
fn glyph_index_in(f: &Face, ch: char) -> Option<usize> {
    let c = ch as u32;
    let gi = if (32..=126).contains(&c) {
        (c - 32) as usize
    } else if let Ok(i) = f.extras.binary_search(&c) {
        95 + i
    } else {
        95 + f.extras.len() + f.wide.binary_search(&c).ok()?
    };
    let &(_, w, _, _, _, adv) = f.glyphs.get(gi)?;
    (w > 0 || adv > 0).then_some(gi)
}

/// Resolve a char to the face that actually carries it and its glyph index
/// (RFC-0082): this face first, then its `fallback`, then the replacement
/// glyph `?`. A codepoint nobody has renders blank — fail-visible, never a
/// substituted shape.
#[inline]
fn resolve(f: &'static Face, ch: char) -> (&'static Face, usize) {
    if let Some(gi) = glyph_index_in(f, ch) {
        return (f, gi);
    }
    if let Some(fb) = f.fallback {
        if let Some(gi) = glyph_index_in(fb, ch) {
            return (fb, gi);
        }
        if let Some(gi) = glyph_index_in(fb, '?') {
            return (fb, gi);
        }
    }
    (f, ('?' as u32 - 32) as usize)
}

#[inline]
fn kern(f: &Face, left: usize, right: usize) -> i32 {
    for &(l, r, k) in f.kern {
        if l as usize == left && r as usize == right {
            return k as i32;
        }
    }
    0
}

/// Blend the slice of the run `text` that intersects surface row `local_y`
/// into `row` (BGRA, straight alpha). The run's band starts at row `top`
/// (`i32`: a band may start above the surface when partially scrolled off),
/// the pen at column `x0` (`i32`: a run may start left of the row — a scroll
/// layer's band drawn a run of columns at a time — and is clipped there);
/// pixels at or beyond `clip_end_x` (and beyond the row buffer) are not
/// touched. Glyphs are placed exactly where [`measure`] counts them, with
/// `tracking` after every glyph. Glyph coverage is blended `src OVER dst`
/// scaled by `color`'s alpha — text composites correctly over glass tints.
#[allow(clippy::too_many_arguments)]
pub fn draw_text_row(
    row: &mut [u8],
    local_y: u32,
    top: i32,
    x0: i32,
    clip_end_x: u32,
    text: impl Iterator<Item = char>,
    size: FontSize,
    tracking: Tracking,
    color: [u8; 4],
) {
    let primary = face(size);
    let band_y = local_y as i32 - top;
    if band_y < 0 || band_y >= primary.line_h as i32 {
        return;
    }
    let row_px = (row.len() / 4) as u32;
    let clip = clip_end_x.min(row_px);
    let mut pen = Pen::new(primary, tracking);
    for ch in text {
        let (f, gi, x) = pen.place(ch);
        let Some(&(off, w, h, left, gtop, _)) = f.glyphs.get(gi) else { continue };
        // A fallback glyph was baked against ITS face's ascent; shifting by
        // the ascent delta puts both faces' baselines on the same row.
        let gy = band_y - (gtop as i32 + primary.ascent - f.ascent);
        if w > 0 && gy >= 0 && (gy as u16) < h {
            let start = off as usize + gy as usize * w as usize;
            if let Some(src) = f.cov().get(start..start + w as usize) {
                let gx = x0.saturating_add(x).saturating_add(i32::from(left));
                for (i, &cov) in src.iter().enumerate() {
                    if cov == 0 {
                        continue;
                    }
                    let px = gx.saturating_add(i as i32);
                    if px < 0 {
                        continue;
                    }
                    let px = px as u32;
                    if px >= clip {
                        break;
                    }
                    blend_px(&mut row[px as usize * 4..px as usize * 4 + 4], color, cov);
                }
            }
        }
        if x0.saturating_add(pen.width()) >= clip as i32 {
            break;
        }
    }
}

/// Blend `color` scaled by `coverage` over one straight-alpha BGRA pixel.
#[inline]
pub(crate) fn blend_px(dst: &mut [u8], color: [u8; 4], coverage: u8) {
    let sa = (coverage as u32 * color[3] as u32 + 127) / 255; // 0..255
    if sa == 0 {
        return;
    }
    let inv = 255 - sa;
    for c in 0..3 {
        dst[c] = ((color[c] as u32 * sa + dst[c] as u32 * inv + 127) / 255) as u8;
    }
    dst[3] = (sa + dst[3] as u32 * inv / 255).min(255) as u8;
}
