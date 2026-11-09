// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::SurfaceBuffer;
use crate::cursor_assets;
use crate::ids::CallerCtx;

pub fn render_cursor_surface(caller: CallerCtx) -> Option<SurfaceBuffer> {
    SurfaceBuffer::from_bgra_pixels(
        caller,
        0x5756_4355_5253_4f52,
        cursor_assets::DIM,
        cursor_assets::DIM,
        // The boot theme is dark: the white pointer (the live pointer follows the theme).
        cursor_assets::WHITE.arrow.to_vec(),
    )
    .ok()
}
