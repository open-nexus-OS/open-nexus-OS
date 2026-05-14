// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::assets;
use crate::buffer::SurfaceBuffer;
use crate::ids::CallerCtx;

pub fn render_cursor_surface(caller: CallerCtx) -> Option<SurfaceBuffer> {
    let output = nexus_svg::render_svg(assets::CURSOR_LEFT_PTR_SVG).ok()?;
    SurfaceBuffer::from_bgra_pixels(
        caller,
        0x5756_4355_5253_4f52,
        output.width,
        output.height,
        output.buffer,
    )
    .ok()
}
