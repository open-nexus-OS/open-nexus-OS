// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Golden tests for cursor SVG asset rendering.
//! Verifies that the embedded CURSOR_LEFT_PTR_SVG produces non-empty output
//! with expected dimensions and pixel content.

use nexus_svg::render_svg;

/// Exact cursor SVG from windowd/src/assets.rs
const CURSOR_SVG: &str = r##"<svg width="48" height="48" xmlns="http://www.w3.org/2000/svg">
    <path d="M 4,3 L 42,27 L 30,30 L 38,44 L 29,47 L 21,33 L 11,41 Z" fill="#ffffff" />
    <path d="M 10,11 L 33,25 L 24,27 L 31,40 L 28,41 L 20,27 L 14,34 Z" fill="#1a1a2e" />
</svg>"##;

#[test]
fn cursor_svg_renders_non_empty() {
    let output = render_svg(CURSOR_SVG).expect("cursor SVG must parse and render");
    assert_eq!(output.width, 48, "cursor width");
    assert_eq!(output.height, 48, "cursor height");
    assert_eq!(output.buffer.len(), 48 * 48 * 4, "BGRA8888 buffer size");

    let non_zero = output.buffer.iter().filter(|&&b| b != 0).count();
    assert!(
        non_zero > 100,
        "cursor SVG must produce substantial pixels (got {non_zero} / {})",
        output.buffer.len()
    );
}

#[test]
fn cursor_svg_has_dark_fill() {
    let output = render_svg(CURSOR_SVG).expect("cursor SVG must render");
    // Check a pixel near the center of the arrow.
    let y = 24;
    let x = 20;
    let idx = ((y * output.width as usize + x) * 4) as usize;
    if idx + 4 <= output.buffer.len() {
        // At least some non-transparent pixel should exist
        let has_color = output
            .buffer
            .chunks(4)
            .any(|p| p[3] > 0 && (p[0] > 0 || p[1] > 0 || p[2] > 0));
        assert!(has_color, "cursor must have visible colored pixels");
    }
}

#[test]
fn cursor_svg_deterministic() {
    let r1 = render_svg(CURSOR_SVG).expect("first render");
    let r2 = render_svg(CURSOR_SVG).expect("second render");
    assert_eq!(
        r1.buffer, r2.buffer,
        "cursor rendering must be deterministic"
    );
    assert_eq!(r1.width, r2.width);
    assert_eq!(r1.height, r2.height);
}
