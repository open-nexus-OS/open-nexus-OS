// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Scene-row compositing for the CPU base scene pass.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via compositor integration tests

use super::cache::{
    BackdropCacheEntry, GlassLayerCache, LayerCache, PathCacheEntry, ShadowBoxCacheEntry,
};
use super::source::copy_scaled_systemui_row_clipped;
use super::surface::draw_proof_surface_row;
use super::types::{RenderClip, SourceFrame};
use super::SHADOW_BOX_CACHE_ENTRIES;
use crate::error::WindowdError;
use crate::live_runtime::{GlassQuality, LayoutHotPathIndex};
use crate::smoke::VisibleBootstrapMode;
use input_live_protocol::VisibleState;
use nexus_effects::ShadowArena;
use nexus_layout::LayoutResult;

pub(crate) fn copy_scene_row(
    blur_row_buf: &mut [u8],
    shadow_scratch: &mut [u8],
    backdrop_cache: &mut [BackdropCacheEntry],
    glass_layer: &mut GlassLayerCache,
    glass_scratch: &mut [u8],
    path_cache: &mut [PathCacheEntry],
    source_frame: &SourceFrame,
    source_x_lut: &[u32],
    source_y_lut: &[u32],
    mode: VisibleBootstrapMode,
    state: VisibleState,
    proof_layout: Option<&LayoutResult>,
    proof_layout_index: Option<&LayoutHotPathIndex>,
    filter_text: &str,
    filtered_words: &[&'static str],
    y: u32,
    render_clip: RenderClip,
    glass_quality: GlassQuality,
    paint_only: bool,
    row: &mut [u8],
    layer_cache: &mut LayerCache,
    shadow_arena: &mut ShadowArena<'_>,
    col_scratch: &mut [u8],
    shadow_box_cache: &mut [ShadowBoxCacheEntry; SHADOW_BOX_CACHE_ENTRIES],
) -> Result<(), WindowdError> {
    // G3 (RFC-0067 P5-Final): Plane 1 now holds ONLY the wallpaper. The proof
    // panel (`combined_panels`) is composited as a GPU layer with its shadow as a
    // layer effect (see `runtime/scene.rs` "1·proof" + `render_proof_surface`), so
    // the per-row CPU shadow (`compute_shadow_row`) and content bake
    // (`draw_proof_surface_row`) are retired from this hot path.
    let _ = (
        proof_layout,
        proof_layout_index,
        filter_text,
        filtered_words,
        shadow_scratch,
        blur_row_buf,
        backdrop_cache,
        glass_layer,
        glass_scratch,
        path_cache,
        glass_quality,
        layer_cache,
        shadow_arena,
        col_scratch,
        shadow_box_cache,
        paint_only,
        &state,
    );
    copy_scaled_systemui_row_clipped(
        source_frame,
        source_x_lut,
        source_y_lut,
        mode,
        y,
        row,
        render_clip,
    )?;
    Ok(())
}
