// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Asset rendering functions for windowd.
//!
//! These functions render embedded assets (cursors, wallpaper, text)
//! onto the display framebuffer.

use crate::assets;
use crate::markers::CURSOR_SVG_LOADED_MARKER;

/// Rasterize the left_ptr cursor SVG into a BGRA8888 pixel buffer.
///
/// Returns `Some((width, height, buffer))` on success, or `None`
/// if the SVG parsing/rasterization failed.
pub fn render_cursor_left_ptr() -> Option<(u32, u32, alloc::vec::Vec<u8>)> {
    let output = nexus_svg::render_svg(assets::CURSOR_LEFT_PTR_SVG).ok()?;
    Some((output.width, output.height, output.buffer))
}
