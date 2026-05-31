// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Scene-row compositing: full-frame row and cursor-background row copy.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via compositor integration tests

use super::cache::{
    BackdropCacheEntry, GlassLayerCache, LayerCache, PathCacheEntry, ShadowBoxCacheEntry,
};
use super::cursor::blend_cursor_row;
use super::shadow::compute_shadow_row;
use super::source::copy_scaled_systemui_row_clipped;
use super::surface::draw_proof_surface_row;
use super::types::{RenderClip, SourceFrame};
use super::SHADOW_BOX_CACHE_ENTRIES;
use crate::error::WindowdError;
use crate::live_runtime::{GlassQuality, LayoutHotPathIndex};
use crate::smoke::VisibleBootstrapMode;
use alloc::vec::Vec;
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
    cursor_bitmap: Option<&[u8]>,
    cursor_width: u32,
    cursor_height: u32,
    cursor_x: i32,
    cursor_y: i32,
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
    copy_scaled_systemui_row_clipped(
        source_frame,
        source_x_lut,
        source_y_lut,
        mode,
        y,
        row,
        render_clip,
    )?;
    if !paint_only {
        compute_shadow_row(
            state,
            proof_layout,
            proof_layout_index,
            y,
            row,
            shadow_scratch,
            blur_row_buf,
            shadow_arena,
            col_scratch,
            shadow_box_cache,
        )?;
    }
    draw_proof_surface_row(
        state,
        proof_layout,
        proof_layout_index,
        filter_text,
        filtered_words,
        y,
        row,
        render_clip,
        backdrop_cache,
        glass_layer,
        glass_scratch,
        path_cache,
        source_frame,
        source_x_lut,
        source_y_lut,
        mode,
        glass_quality,
        blur_row_buf,
        layer_cache,
        paint_only,
    )?;
    if let Some(cursor_bitmap) = cursor_bitmap {
        blend_cursor_row(
            row,
            y,
            cursor_bitmap,
            cursor_width,
            cursor_height,
            cursor_x - crate::assets::CURSOR_HOTSPOT_X,
            cursor_y - crate::assets::CURSOR_HOTSPOT_Y,
        );
    }
    Ok(())
}

pub(crate) fn copy_cursor_background_row(
    blur_row_buf: &mut [u8],
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
    row: &mut [u8],
    layer_cache: &mut LayerCache,
    shadow_scratch: &mut [u8],
    shadow_arena: &mut ShadowArena<'_>,
    col_scratch: &mut [u8],
    shadow_box_cache: &mut [ShadowBoxCacheEntry; SHADOW_BOX_CACHE_ENTRIES],
) -> Result<(), WindowdError> {
    copy_scaled_systemui_row_clipped(
        source_frame,
        source_x_lut,
        source_y_lut,
        mode,
        y,
        row,
        render_clip,
    )?;
    compute_shadow_row(
        state,
        proof_layout,
        proof_layout_index,
        y,
        row,
        shadow_scratch,
        blur_row_buf,
        shadow_arena,
        col_scratch,
        shadow_box_cache,
    )?;
    draw_proof_surface_row(
        state,
        proof_layout,
        proof_layout_index,
        filter_text,
        filtered_words,
        y,
        row,
        render_clip,
        backdrop_cache,
        glass_layer,
        glass_scratch,
        path_cache,
        source_frame,
        source_x_lut,
        source_y_lut,
        mode,
        GlassQuality::High,
        blur_row_buf,
        layer_cache,
        false,
    )
}
