// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Runtime text rendering — since the RFC-0067-P5 promotion this is
//! a thin client of the shared text SSOT (`nexus-text-baked`): the baked A8
//! atlases, measurement and the row-based glyph blender moved there VERBATIM
//! so windowd, the app-host runtime, and future DSL shells draw text
//! identically. This module keeps the crate-internal names stable and names
//! the one text style windowd measures itself (RFC-0116 `body`).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests live in nexus-text-baked (promoted with the code); the body
//!   style's lockstep with the typed tokens below

pub(crate) use nexus_text_baked::{advance, FontSize, Tracking};

/// The `body` text style's face (RFC-0116: 14 px, weight 400).
pub(crate) const BODY: FontSize = FontSize::S14W400;
/// The `body` text style's tracking (none).
pub(crate) const BODY_TRACKING: Tracking = Tracking::NONE;
/// The `body` text style's line box (20 px). The face's own band
/// (`nexus_text_baked::line_height(BODY)`, 17 px) is shorter: a painter centres it in the line
/// with half-leading, `top + (BODY_LINE − line_height(BODY)) / 2`.
pub(crate) const BODY_LINE: u32 = 20;

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::FontWeight;
    use nexus_text_baked::{line_height, Weight};
    use nexus_theme_tokens::TextStyleToken;

    /// The baked weight a style weight names.
    fn baked(weight: FontWeight) -> Weight {
        match weight {
            FontWeight::Light => Weight::Light,
            FontWeight::Normal => Weight::Regular,
            FontWeight::Medium => Weight::Medium,
            FontWeight::Semibold => Weight::SemiBold,
            FontWeight::Bold => Weight::Bold,
        }
    }

    /// The consts are the typed token's `body` (the theme's `[textStyle.body]`), so a change
    /// to the style fails here instead of drifting the chat geometry.
    #[test]
    fn the_body_consts_are_the_body_style() {
        let spec = TextStyleToken::Body.spec();
        assert_eq!(FontSize::exact(i32::from(spec.size), baked(spec.weight)), Some(BODY));
        assert_eq!(BODY_TRACKING, Tracking::em(spec.tracking_milli_em));
        assert_eq!(BODY_LINE, u32::from(spec.line));
        assert!(line_height(BODY) <= BODY_LINE, "the face's band fits the style's line box");
    }
}
