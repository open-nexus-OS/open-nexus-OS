// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::assets;
use crate::buffer::SurfaceBuffer;
use crate::ids::CallerCtx;

pub fn render_cursor_surface(caller: CallerCtx) -> Option<SurfaceBuffer> {
    SurfaceBuffer::from_bgra_pixels(
        caller,
        0x5756_4355_5253_4f52,
        assets::CURSOR_LEFT_PTR_WIDTH,
        assets::CURSOR_LEFT_PTR_HEIGHT,
        assets::CURSOR_LEFT_PTR_BGRA.to_vec(),
    )
    .ok()
}
