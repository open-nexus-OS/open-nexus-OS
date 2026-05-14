// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_arguments,
    clippy::useless_vec,
    clippy::needless_borrow,
    clippy::manual_contains
)]

//! CONTEXT: BreezeX cursor pipeline for TASK-0057 / RFC-0056.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: See tests/ directory
//!
//! PUBLIC API:
//!   - `CursorSet`: loads a BreezeX cursor theme directory.
//!   - `CursorAsset`: rasterized cursor bitmap + hotspot + frames.
//!
//! DEPENDENCIES:
//!   - `nexus-svg`: SVG parsing + rasterization for cursor SVGs.
//!
//! ADR: docs/rfcs/RFC-0056-ui-v2b-asset-theme-cursor-text-pipeline.md

mod hotspot;
mod load;

pub use hotspot::hotspot_for;
pub use load::{load_cursor_set, CursorAsset, CursorSet};
