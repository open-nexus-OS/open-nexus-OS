// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Golden tests for cursor SVG asset rendering.
//! Verifies that the embedded CURSOR_LEFT_PTR_SVG produces non-empty output
//! with expected dimensions and pixel content.

use nexus_svg::render_svg;

/// Normalized Mocu `default.svg` left pointer used by windowd.
const CURSOR_SVG: &str = r##"<svg width="32" height="32" xmlns="http://www.w3.org/2000/svg">
<path d="M 1 1 L 1 30 L 9.3 26.6 L 12.9 35.3 L 18 33 L 14.4 24.3 L 22.7 20.9 Z" fill="#0a0b0c"/>
<path d="M 2 2 L 2 29.5 L 9.9 26.2 L 13.3 34.6 L 17 33 L 13.6 24.7 L 21.5 21.4 Z" fill="#1a1b1c"/>
<path d="M 4 4 L 4 25 L 10.4 22.4 L 13.8 30.6 L 15.2 30 L 11.9 21.9 L 18.4 19.2 Z" fill="#fafbfc"/>
</svg>"##;

#[test]
fn cursor_svg_renders_non_empty() {
    let output = render_svg(CURSOR_SVG).expect("cursor SVG must parse and render");
    assert_eq!(output.width, 32, "cursor width");
    assert_eq!(output.height, 32, "cursor height");
    assert_eq!(output.buffer.len(), 32 * 32 * 4, "BGRA8888 buffer size");

    let non_zero = output.buffer.iter().filter(|&&b| b != 0).count();
    assert!(
        non_zero > 100,
        "cursor SVG must produce substantial pixels (got {non_zero} / {})",
        output.buffer.len()
    );
}

#[test]
fn cursor_svg_has_mocu_stroke_and_fill() {
    let output = render_svg(CURSOR_SVG).expect("cursor SVG must render");
    let has_white_fill = output.buffer.chunks(4).any(|p| p[3] > 0 && p[0] > 0xf0);
    let has_dark_stroke = output
        .buffer
        .chunks(4)
        .any(|p| p[3] > 0 && p[0] < 0x30 && p[1] < 0x30 && p[2] < 0x30);
    assert!(
        has_white_fill,
        "Mocu cursor must keep the white #fafbfc fill"
    );
    assert!(
        has_dark_stroke,
        "Mocu cursor must keep the dark #1a1b1c stroke/shadow"
    );
}

#[test]
fn cursor_svg_uses_sensible_32px_extents() {
    let output = render_svg(CURSOR_SVG).expect("cursor SVG must render");
    let mut min_x = output.width;
    let mut min_y = output.height;
    let mut max_x = 0;
    let mut max_y = 0;
    for y in 0..output.height {
        for x in 0..output.width {
            let idx = ((y * output.width + x) * 4) as usize;
            if output.buffer[idx + 3] == 0 {
                continue;
            }
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    assert!(max_x > min_x, "cursor must render visible pixels");
    assert!(max_y > min_y, "cursor must render visible pixels");
    assert!(
        max_x - min_x >= 18,
        "32px cursor must not contain a collapsed glyph"
    );
    assert!(
        max_y - min_y >= 26,
        "32px cursor must use a practical screen cursor height"
    );
}

#[test]
fn cursor_svg_fill_stays_inside_mocu_outline() {
    let output = render_svg(CURSOR_SVG).expect("cursor SVG must render");
    let mut dark_min_x = output.width;
    let mut dark_min_y = output.height;
    let mut dark_max_x = 0;
    let mut dark_max_y = 0;
    let mut white_min_x = output.width;
    let mut white_min_y = output.height;
    let mut white_max_x = 0;
    let mut white_max_y = 0;
    for y in 0..output.height {
        for x in 0..output.width {
            let idx = ((y * output.width + x) * 4) as usize;
            let pixel = &output.buffer[idx..idx + 4];
            if pixel[3] == 0 {
                continue;
            }
            if pixel[0] < 0x30 && pixel[1] < 0x30 {
                dark_min_x = dark_min_x.min(x);
                dark_min_y = dark_min_y.min(y);
                dark_max_x = dark_max_x.max(x);
                dark_max_y = dark_max_y.max(y);
            } else if pixel[0] > 0xf0 {
                white_min_x = white_min_x.min(x);
                white_min_y = white_min_y.min(y);
                white_max_x = white_max_x.max(x);
                white_max_y = white_max_y.max(y);
            }
        }
    }
    assert!(
        white_min_x > dark_min_x,
        "white fill must be inset from left outline"
    );
    assert!(
        white_min_y > dark_min_y,
        "white fill must be inset from top outline"
    );
    assert!(
        white_max_x < dark_max_x,
        "white fill must be inset from right outline"
    );
    assert!(
        white_max_y < dark_max_y,
        "white fill must be inset from bottom outline"
    );
    let dark_center_x = (dark_min_x + dark_max_x) / 2;
    let white_center_x = (white_min_x + white_max_x) / 2;
    assert!(
        dark_center_x.abs_diff(white_center_x) <= 4,
        "white fill should not be visibly offset from the Mocu outline"
    );
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
