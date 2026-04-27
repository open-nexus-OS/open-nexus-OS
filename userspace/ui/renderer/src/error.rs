// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Stable renderer error classes for bounds and proof-honest rejects.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 renderer integration tests, 24 ui_host_snap contract tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use std::error::Error;
use std::fmt;

pub type RenderResult<T> = Result<T, RenderError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "renderer errors must be handled"]
pub enum RenderError {
    InvalidDimensions,
    InvalidStride,
    FrameTooLarge,
    ImageTooLarge,
    GlyphRunTooLarge,
    ArithmeticOverflow,
    InvalidRect,
    DamageOverflow,
    FixtureFontRejected,
    Unsupported,
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::InvalidDimensions => "invalid_dimensions",
            Self::InvalidStride => "invalid_stride",
            Self::FrameTooLarge => "frame_too_large",
            Self::ImageTooLarge => "image_too_large",
            Self::GlyphRunTooLarge => "glyph_run_too_large",
            Self::ArithmeticOverflow => "arithmetic_overflow",
            Self::InvalidRect => "invalid_rect",
            Self::DamageOverflow => "damage_overflow",
            Self::FixtureFontRejected => "fixture_font_rejected",
            Self::Unsupported => "unsupported",
        };
        f.write_str(label)
    }
}

impl Error for RenderError {}
