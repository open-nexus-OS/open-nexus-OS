// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![deny(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_arguments,
    clippy::useless_vec,
    clippy::needless_borrow
)]

//! CONTEXT: HarfBuzz-compatible text shaping for TASK-0057 / RFC-0056.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: See tests/ directory
//!
//! PUBLIC API:
//!   - `ShapeContext`: loads fonts, shapes text via rustybuzz.
//!   - `GlyphRun`: shaped glyph positions + cluster map.
//!   - `FontId`, `GlyphIndex`, `PixelSize`: newtype wrappers.
//!
//! DEPENDENCIES:
//!   - `rustybuzz`: pure-Rust HarfBuzz port for shaping.
//!   - `fontdue`: pure-Rust font parser (for glyph rasterization in Phase 2b).
//!
//! ADR: docs/rfcs/RFC-0056-ui-v2b-asset-theme-cursor-text-pipeline.md

mod context;
mod error;
mod types;
mod variation;
mod wrap;
mod cache;

pub use context::ShapeContext;
pub use error::{ShapeError, ShapeResult};
pub use types::{
    FontId, GlyphBitmap, GlyphIndex, GlyphRun, PixelSize, VariationAxis, VariationSettings,
};
