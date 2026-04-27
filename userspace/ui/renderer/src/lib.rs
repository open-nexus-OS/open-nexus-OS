// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: Host-first BGRA8888 CPU renderer for TASK-0054.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 renderer integration tests, 24 ui_host_snap contract tests
//!
//! PUBLIC API:
//!   - `Frame`: owned BGRA8888 framebuffer with checked allocation.
//!   - `Damage`: bounded dirty-rect accumulator.
//!   - `Image`: checked in-memory BGRA source image.
//!   - `FixtureFont`: repo-owned deterministic fixture font.
//!   - `PixelBgra`, `Point`, `Rect`, checked dimension/stride newtypes.
//!
//! DEPENDENCIES:
//!   - `std`: host-first allocation and error integration.
//!
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

pub mod damage;
pub mod error;
pub mod font;
pub mod frame;
pub mod geometry;
pub mod image;
pub mod limits;
pub mod math;
pub mod pixel;
pub mod primitives;
pub mod units;

pub use damage::Damage;
pub use error::{RenderError, RenderResult};
pub use font::{FixtureFont, Glyph};
pub use frame::Frame;
pub use geometry::{Point, Rect};
pub use image::Image;
pub use limits::{
    BYTES_PER_PIXEL, MAX_DAMAGE_RECTS, MAX_FRAME_HEIGHT, MAX_FRAME_PIXELS, MAX_FRAME_WIDTH,
    MAX_GLYPHS, MAX_IMAGE_HEIGHT, MAX_IMAGE_PIXELS, MAX_IMAGE_WIDTH, STRIDE_ALIGNMENT,
};
pub use pixel::PixelBgra;
pub use units::{
    DamageRectCount, ImageHeight, ImageWidth, StrideBytes, SurfaceHeight, SurfaceWidth,
};
