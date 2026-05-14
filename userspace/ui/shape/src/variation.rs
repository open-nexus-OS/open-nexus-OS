// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Variable font (OpenType Font Variations) coordinate application.
//!
//! Applies `VariationSettings` to a `rustybuzz::Face` by accessing the
//! underlying `ttf_parser::Face` via `AsMut`. Fonts without matching axes
//! silently ignore the requested coordinates (standard variable-font behaviour).

use crate::types::VariationSettings;

/// Apply variation coordinates from `settings` to the given face.
///
/// Each axis in `settings` is converted to a `ttf_parser::Tag` and set
/// individually via `set_variation()`. Axes not present in the font
/// return `None` and are silently skipped.
pub fn apply_to_face(face: &mut rustybuzz::Face, settings: &VariationSettings) {
    use core::convert::AsMut;

    let ttfp_face: &mut ttf_parser::Face = face.as_mut();

    for axis in &settings.axes {
        let tag = ttf_parser::Tag::from_bytes(&axis.tag);
        // Per-axis: set_variation returns None if axis not in font — OK to ignore.
        ttfp_face.set_variation(tag, axis.value);
    }
}
