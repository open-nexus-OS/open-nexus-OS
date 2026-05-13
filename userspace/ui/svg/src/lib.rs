// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: SVG rich subset renderer for TASK-0057 / RFC-0056.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: See tests/ directory
//!
//! PUBLIC API:
//!   - `render_svg`: parse + rasterize an SVG string → BGRA8888 buffer.
//!   - `SvgDocument`: parsed SVG element tree.
//!   - `SvgError`: error types for parse/render failures.
//!
//! Allowed elements: svg, g, path, rect, circle, ellipse, line, polygon,
//!   defs, linearGradient, stop, and basic transforms.
//! Rejected: script, foreignObject, use (external), filter, animate,
//!   external references, data: URIs.
//!
//! DEPENDENCIES: none (hand-written XML tokenizer). Host-first; no_std ready.
//!
//! ADR: docs/rfcs/RFC-0056-ui-v2b-asset-theme-cursor-text-pipeline.md

mod elements;
mod error;
mod limits;
mod parse;
mod raster;
mod tessellate;

pub use elements::{PathCommand, PathData, SvgDocument, SvgElement, Transform};
pub use error::{SvgError, SvgResult};
pub use limits::{MAX_PATH_SEGMENTS, MAX_SVG_DIMENSION, MAX_SVG_NODES, OUTPUT_BYTES_PER_PIXEL};
pub use parse::parse_svg;
pub use raster::{rasterize_document, RasterOutput};

/// Parse an SVG string and rasterize it to a BGRA8888 buffer.
///
/// The output width and height are taken from the SVG `<svg>` element's
/// `width` and `height` attributes (pixel units only).
///
/// # Errors
///
/// Returns `SvgError` if parsing fails (unsupported elements, malformed XML,
/// limits exceeded) or if rasterization fails.
pub fn render_svg(svg_str: &str) -> SvgResult<RasterOutput> {
    let doc = parse_svg(svg_str)?;
    rasterize_document(&doc)
}
