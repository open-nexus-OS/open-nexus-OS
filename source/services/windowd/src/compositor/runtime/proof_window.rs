// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: G3 (RFC-0067 P5-Final) — render the proof panel (`combined_panels`)
//! content into its retained atlas surface so it can be composited as a GPU
//! LAYER (with a soft drop shadow as a layer effect) instead of CPU-baked into
//! Plane 1. Mirrors `render_chat_surface`: the panel becomes a layer like the
//! chat/search windows, which lets Plane 1 hold only the wallpaper and the CPU
//! shadow pass (`compute_shadow_row`) retire.
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: boot-verified (virgl/mmio)

use super::*;

impl DisplayServerRuntime {
    /// Render the proof panel's content into `proof_atlas` (full-stride rows; the
    /// panel occupies columns `SCENE_ORIGIN_X..`). Surface row `k` holds the
    /// panel's content at display row `SCENE_ORIGIN_Y + k`, over a transparent
    /// background — so the composite samples the panel sub-rect and the shadow
    /// blends over the wallpaper. Re-rendered only when the panel content changes
    /// (filter/scroll); scrolling within is a composite offset (G3c).
    pub(super) fn render_proof_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let panel_y: u32 = SCENE_ORIGIN_Y as u32;
        let panel_h = (crate::proof_panel_spec::PANEL_HEIGHT
            .max(crate::proof_panel_spec::FILTER_PANEL_HEIGHT))
        .max(1) as u32;
        let abs_row = self.proof_atlas.abs_row;
        let active_filter_idx = self.active_filter_idx;
        // Disjoint field borrows (mirrors the present-band split in present.rs).
        let mode = self.mode;
        let state = self.state;
        let render_clip = RenderClip::full(mode.width);
        let source_frame = &self.source_frame;
        let source_x_lut = self.source_x_lut.as_slice();
        let source_y_lut = self.source_y_lut.as_slice();
        let filtered_words = self.filtered_words.as_slice();
        let filter_text = state.text_input();
        let proof_layout = self.proof_layouts.as_ref().and_then(|l| l.get(active_filter_idx));
        let proof_layout_index = self.proof_layout_index.as_ref();
        let backdrop_cache = &mut self.backdrop_cache;
        let glass_layer = &mut self.glass_layer;
        let glass_scratch = &mut self.glass_scratch;
        let path_cache = &mut self.path_cache;
        let blur_row_buf = &mut self.blur_row_buf[..stride];
        let layer_cache = &mut self.layer_cache;
        let band = &mut self.band_scratch;
        let mut k = 0u32;
        while k < panel_h {
            let band_end = (k + ROW_WRITE_CHUNK as u32).min(panel_h);
            let band_rows = (band_end - k) as usize;
            for (i, ky) in (k..band_end).enumerate() {
                let row = &mut band[i * stride..(i + 1) * stride];
                // Transparent base: only the panel's own pixels are written below,
                // so the composite reveals the wallpaper outside the panel shape.
                row.fill(0);
                crate::compositor::surface::draw_proof_surface_row(
                    state,
                    proof_layout,
                    proof_layout_index,
                    filter_text,
                    filtered_words,
                    panel_y + ky,
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
                )?;
            }
            let dst = (abs_row + k) as usize * stride;
            vmo_write(handle, dst, &band[..band_rows * stride])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            k = band_end;
        }
        Ok(())
    }
}
