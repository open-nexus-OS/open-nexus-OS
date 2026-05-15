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
mod limits;
mod math;
mod parse;
mod raster;
mod tessellate;

pub use elements::{PathCommand, PathData, SvgDocument, SvgElement, Transform};
pub use error::{SvgError, SvgResult};
pub use limits::{MAX_PATH_SEGMENTS, MAX_SVG_DIMENSION, MAX_SVG_NODES, OUTPUT_BYTES_PER_PIXEL};
pub use parse::parse_svg;
pub use raster::{rasterize_document, RasterOutput};

pub fn render_svg(svg_str: &str) -> SvgResult<RasterOutput> {
    let doc = parse_svg(svg_str)?;
    rasterize_document(&doc)
}
