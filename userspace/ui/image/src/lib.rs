// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: PNG/JPG decode + scale pipeline for TASK-0057 / RFC-0056.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: See tests/ directory
//!
//! PUBLIC API:
//!   - `decode_image`: PNG or JPG bytes → RGBA8 buffer with dimensions.
//!   - `scale_image`: bilinear or nearest-neighbor downscale/upscale.
//!   - `DecodedImage`: owned RGBA8 image with width/height.
//!
//! DEPENDENCIES:
//!   - `png`: PNG decoding (host-first).
//!   - `jpeg-decoder`: JPEG decoding (host-first).
//!   OS path will use no_std alternatives or pre-decoded bitmaps.
//!
//! ADR: docs/rfcs/RFC-0056-ui-v2b-asset-theme-cursor-text-pipeline.md

mod decode;
mod error;
mod limits;
mod scale;

pub use decode::{decode_image, DecodedImage, ImageFormat};
pub use error::{ImageError, ImageResult};
pub use limits::{MAX_DECODE_PIXELS, MAX_IMAGE_DIMENSION};
pub use scale::{scale_image, ScaleFilter};
