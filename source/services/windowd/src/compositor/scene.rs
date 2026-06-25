// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Scene-row compositing for the CPU base scene pass.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via compositor integration tests

use super::source::copy_scaled_systemui_row_clipped;
use super::types::{RenderClip, SourceFrame};
use crate::error::WindowdError;
use crate::smoke::VisibleBootstrapMode;

/// CPU base-scene pass for one retained-plane row. Since RFC-0067 P5-Final (G3),
/// Plane 1 holds ONLY the wallpaper — the proof panel and all glass are GPU
/// layers (see `runtime/scene.rs`) — so this is now just the SystemUI/wallpaper
/// scale-copy. The per-row CPU shadow + proof-content bake were retired.
pub(crate) fn copy_scene_row(
    source_frame: &SourceFrame,
    source_x_lut: &[u32],
    source_y_lut: &[u32],
    mode: VisibleBootstrapMode,
    y: u32,
    render_clip: RenderClip,
    row: &mut [u8],
) -> Result<(), WindowdError> {
    copy_scaled_systemui_row_clipped(
        source_frame,
        source_x_lut,
        source_y_lut,
        mode,
        y,
        row,
        render_clip,
    )
}
