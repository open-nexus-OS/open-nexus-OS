// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Asset drawing functions for the BGRA8888 renderer.
//!
//! These functions blit external asset types (images, SVGs, glyph runs)
//! onto a renderer `Frame` using the checked pixel API.

use crate::frame::Frame;
use crate::pixel::PixelBgra;

/// Blit a decoded image onto a frame at the given position.
/// Pixels outside the frame bounds are silently clipped.
pub fn draw_image(frame: &mut Frame, x: i32, y: i32, image: &nexus_image::DecodedImage) {
    let iw = image.width as i32;
    let ih = image.height as i32;

    for row in 0..ih {
        let fy = y + row;
        for col in 0..iw {
            let fx = x + col;
            let si = ((row as u32 * image.width + col as u32) * 4) as usize;
            let b = image.data[si];
            let g = image.data[si + 1];
            let r = image.data[si + 2];
            let a = image.data[si + 3];
            if a == 0 {
                continue;
            }
            // set_pixel_checked silently clips out-of-bounds
            let _ = frame.set_pixel_checked(fx, fy, PixelBgra::new(b, g, r, a));
        }
    }
}

/// Render an SVG onto a frame at the given position.
pub fn draw_svg(frame: &mut Frame, x: i32, y: i32, svg_output: &nexus_svg::RasterOutput) {
    let sw = svg_output.width as i32;
    let sh = svg_output.height as i32;

    for row in 0..sh {
        let fy = y + row;
        for col in 0..sw {
            let fx = x + col;
            let si = ((row as u32 * svg_output.width + col as u32) * 4) as usize;
            let b = svg_output.buffer[si];
            let g = svg_output.buffer[si + 1];
            let r = svg_output.buffer[si + 2];
            let a = svg_output.buffer[si + 3];
            if a == 0 {
                continue;
            }
            let _ = frame.set_pixel_checked(fx, fy, PixelBgra::new(b, g, r, a));
        }
    }
}

/// Draw a shaped glyph run at the given baseline position.
///
/// Each glyph's bitmap is rasterized via fontdue + GlyphCache
/// (integrated in Phase 2c proof surface).
pub fn draw_glyph_run(_frame: &mut Frame, _x: i32, _y: i32, _run: &nexus_shape::GlyphRun) {
    // Glyph rasterization via fontdue + GlyphCache is integrated in Phase 2c.
    // This stub exists so rendering code can compile against the API.
}
