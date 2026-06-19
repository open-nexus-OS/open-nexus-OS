// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SVG rich subset renderer for TASK-0057 / RFC-0056.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: See tests/ directory

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_arguments,
    clippy::useless_vec,
    clippy::needless_borrow
)]

#[macro_use]
extern crate alloc;

mod elements;
mod error;
mod gradient;
mod limits;
mod math;
mod parse;
mod raster;
mod tessellate;

pub use elements::{PathCommand, PathData, SvgDocument, SvgElement, Transform};
pub use error::{SvgError, SvgResult};
pub use limits::{MAX_PATH_SEGMENTS, MAX_SVG_DIMENSION, MAX_SVG_NODES, OUTPUT_BYTES_PER_PIXEL};
pub use parse::parse_svg;
pub use raster::{rasterize_document, rasterize_document_at, RasterOutput};

pub fn render_svg(svg_str: &str) -> SvgResult<RasterOutput> {
    let doc = parse_svg(svg_str)?;
    rasterize_document(&doc)
}

/// Render a monochrome icon, resolving `currentColor` to `tint` (`(r, g, b)`).
/// Lets the theme drive icon color (Lucide et al. use `stroke="currentColor"`)
/// without editing the SVG source.
pub fn render_svg_tinted(svg_str: &str, tint: (u8, u8, u8)) -> SvgResult<RasterOutput> {
    let color = elements::Color { r: tint.0, g: tint.1, b: tint.2, a: 255 };
    let doc = parse::parse_svg_tinted(svg_str, color)?;
    rasterize_document(&doc)
}

/// Render at an explicit pixel size (HiDPI/5K), scaling the SVG to fit — the
/// asset-pipeline entry for crisp cursor/icon bitmaps at any density.
pub fn render_svg_at(svg_str: &str, width: u32, height: u32) -> SvgResult<RasterOutput> {
    let doc = parse_svg(svg_str)?;
    rasterize_document_at(&doc, width, height)
}

/// Render at an explicit pixel size with `currentColor` resolved to `tint` —
/// themed, HiDPI monochrome icons in one call.
pub fn render_svg_tinted_at(
    svg_str: &str,
    tint: (u8, u8, u8),
    width: u32,
    height: u32,
) -> SvgResult<RasterOutput> {
    let color = elements::Color { r: tint.0, g: tint.1, b: tint.2, a: 255 };
    let doc = parse::parse_svg_tinted(svg_str, color)?;
    rasterize_document_at(&doc, width, height)
}
