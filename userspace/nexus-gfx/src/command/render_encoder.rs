// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::command::buffer::{Command, CommandBuffer, RgbaColor};
use crate::command::layer::{BackdropCache, Layer};
use crate::core::error::GfxError;
use crate::core::types::TileRect;

/// Records rendering commands into a CommandBuffer.
/// Created by CommandBuffer::begin_render_pass().
pub struct RenderCommandEncoder<'a> {
    cmd: &'a mut CommandBuffer,
    active: bool,
}

impl<'a> RenderCommandEncoder<'a> {
    pub(crate) fn new(cmd: &'a mut CommandBuffer) -> Self {
        Self { cmd, active: true }
    }

    // ── Fragment data ────────────────────────────────────────

    pub fn set_fragment_bytes(&mut self, offset: usize, data: &[u8]) {
        self.try_set_fragment_bytes(offset, data).expect("invalid fragment bytes");
    }

    pub fn try_set_fragment_bytes(&mut self, offset: usize, data: &[u8]) -> Result<(), GfxError> {
        if !self.active {
            return Err(GfxError::CommandRejected);
        }
        self.cmd.push_command(Command::SetFragmentBytes { offset, data: data.to_vec() })
    }

    // ── Tiles ─────────────────────────────────────────────────

    pub fn draw_tiles(&mut self, tiles: &[TileRect], color: RgbaColor) {
        self.try_draw_tiles(tiles, color).expect("invalid tile draw");
    }

    pub fn try_draw_tiles(&mut self, tiles: &[TileRect], color: RgbaColor) -> Result<(), GfxError> {
        if !self.active {
            return Err(GfxError::CommandRejected);
        }
        self.cmd.push_command(Command::DrawTiles { tiles: tiles.to_vec(), color })
    }

    // ── Blit ──────────────────────────────────────────────────

    /// Copy a rectangle from the source surface to the framebuffer.
    pub fn blit_surface(
        &mut self,
        src_x: u32,
        src_y: u32,
        dst_x: u32,
        dst_y: u32,
        width: u32,
        height: u32,
    ) {
        self.try_blit_surface(src_x, src_y, dst_x, dst_y, width, height).expect("invalid blit");
    }

    pub fn try_blit_surface(
        &mut self,
        src_x: u32,
        src_y: u32,
        dst_x: u32,
        dst_y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), GfxError> {
        if !self.active {
            return Err(GfxError::CommandRejected);
        }
        self.cmd.push_command(Command::BlitSurface { src_x, src_y, dst_x, dst_y, width, height })
    }

    /// Blit between arbitrary absolute VMO rows — no display_y_offset adjustment.
    /// Used to write/read the Plane 3 blur cache.
    pub fn try_blit_absolute(
        &mut self,
        src_x: u32,
        src_y_abs: u32,
        dst_x: u32,
        dst_y_abs: u32,
        width: u32,
        height: u32,
    ) -> Result<(), GfxError> {
        if !self.active {
            return Err(GfxError::CommandRejected);
        }
        self.cmd.push_command(Command::BlitAbsolute {
            src_x,
            src_y_abs,
            dst_x,
            dst_y_abs,
            width,
            height,
        })
    }

    // ── SDF rounded rect ──────────────────────────────────────

    /// Fill an SDF rounded rectangle with a solid color.
    pub fn fill_sdf_rounded_rect(&mut self, rect: TileRect, radius: u32, color: RgbaColor) {
        self.try_fill_sdf_rounded_rect(rect, radius, color).expect("invalid SDF fill");
    }

    pub fn try_fill_sdf_rounded_rect(
        &mut self,
        rect: TileRect,
        radius: u32,
        color: RgbaColor,
    ) -> Result<(), GfxError> {
        if !self.active {
            return Err(GfxError::CommandRejected);
        }
        self.cmd.push_command(Command::FillSdfRoundedRect { rect, radius, color })
    }

    // ── SDF gradient fill ─────────────────────────────────────

    /// Fill an SDF rounded rectangle with a vertical linear gradient.
    pub fn fill_sdf_gradient(
        &mut self,
        rect: TileRect,
        radius: u32,
        color_top: RgbaColor,
        color_bottom: RgbaColor,
    ) {
        self.try_fill_sdf_gradient(rect, radius, color_top, color_bottom)
            .expect("invalid SDF gradient fill");
    }

    pub fn try_fill_sdf_gradient(
        &mut self,
        rect: TileRect,
        radius: u32,
        color_top: RgbaColor,
        color_bottom: RgbaColor,
    ) -> Result<(), GfxError> {
        if !self.active {
            return Err(GfxError::CommandRejected);
        }
        self.cmd.push_command(Command::FillSdfGradient { rect, radius, color_top, color_bottom })
    }

    // ── Drop shadow ───────────────────────────────────────────

    /// Soft drop shadow behind a rounded rect (SDF falloff over `blur` px).
    pub fn drop_shadow(
        &mut self,
        rect: TileRect,
        radius: u32,
        blur: u32,
        offset_x: i32,
        offset_y: i32,
        color: RgbaColor,
    ) {
        self.try_drop_shadow(rect, radius, blur, offset_x, offset_y, color)
            .expect("invalid drop shadow");
    }

    pub fn try_drop_shadow(
        &mut self,
        rect: TileRect,
        radius: u32,
        blur: u32,
        offset_x: i32,
        offset_y: i32,
        color: RgbaColor,
    ) -> Result<(), GfxError> {
        if !self.active {
            return Err(GfxError::CommandRejected);
        }
        self.cmd.push_command(Command::DropShadow { rect, radius, blur, offset_x, offset_y, color })
    }

    // ── GPU-composited layer (compositor) ─────────────────────

    /// Composite a content-texture layer into the scanout RT with per-layer
    /// GPU effects. See [`Command::CompositeLayer`]. GPU/virgl path only.
    #[allow(clippy::too_many_arguments)]
    pub fn composite_layer(
        &mut self,
        src_row_abs: u32,
        src_x: u32,
        width: u32,
        height: u32,
        dst_x: u32,
        dst_y: u32,
        opacity: u32,
        corner_radius: u32,
        shadow_blur: u32,
        shadow_offset_y: i32,
        shadow_alpha: u32,
        backdrop_blur: u32,
    ) {
        self.try_composite_layer(
            src_row_abs,
            src_x,
            width,
            height,
            dst_x,
            dst_y,
            opacity,
            corner_radius,
            shadow_blur,
            shadow_offset_y,
            shadow_alpha,
            backdrop_blur,
        )
        .expect("invalid composite layer");
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_composite_layer(
        &mut self,
        src_row_abs: u32,
        src_x: u32,
        width: u32,
        height: u32,
        dst_x: u32,
        dst_y: u32,
        opacity: u32,
        corner_radius: u32,
        shadow_blur: u32,
        shadow_offset_y: i32,
        shadow_alpha: u32,
        backdrop_blur: u32,
    ) -> Result<(), GfxError> {
        self.try_composite_layer_tagged(
            src_row_abs,
            src_x,
            width,
            height,
            dst_x,
            dst_y,
            opacity,
            corner_radius,
            shadow_blur,
            shadow_offset_y,
            shadow_alpha,
            backdrop_blur,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        )
    }

    /// Composite a **scrollable** layer under `scroll_id` (non-zero) — the
    /// backend retains it and re-samples it at the id's source-row override on a
    /// lightweight scroll command (the GPU scroll fast path), instead of the
    /// embedder re-composing per frame.
    #[allow(clippy::too_many_arguments)]
    pub fn try_composite_layer_scrollable(
        &mut self,
        src_row_abs: u32,
        src_x: u32,
        width: u32,
        height: u32,
        dst_x: u32,
        dst_y: u32,
        opacity: u32,
        corner_radius: u32,
        shadow_blur: u32,
        shadow_offset_y: i32,
        shadow_alpha: u32,
        backdrop_blur: u32,
        scroll_id: u32,
    ) -> Result<(), GfxError> {
        self.try_composite_layer_tagged(
            src_row_abs,
            src_x,
            width,
            height,
            dst_x,
            dst_y,
            opacity,
            corner_radius,
            shadow_blur,
            shadow_offset_y,
            shadow_alpha,
            backdrop_blur,
            scroll_id,
            0,
            0,
            0,
            0,
            0,
            0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn try_composite_layer_tagged(
        &mut self,
        src_row_abs: u32,
        src_x: u32,
        width: u32,
        height: u32,
        dst_x: u32,
        dst_y: u32,
        opacity: u32,
        corner_radius: u32,
        shadow_blur: u32,
        shadow_offset_y: i32,
        shadow_alpha: u32,
        backdrop_blur: u32,
        scroll_id: u32,
        content_w: u32,
        content_h: u32,
        scroll_band_top_abs: u32,
        scroll_band_h: u32,
        layer_id: u32,
        content_epoch: u32,
    ) -> Result<(), GfxError> {
        if !self.active {
            return Err(GfxError::CommandRejected);
        }
        self.cmd.push_command(Command::CompositeLayer {
            src_row_abs,
            src_x,
            width,
            height,
            dst_x,
            dst_y,
            opacity,
            corner_radius,
            shadow_blur,
            shadow_offset_y,
            shadow_alpha,
            backdrop_blur,
            scroll_id,
            content_w,
            content_h,
            scroll_band_top_abs,
            scroll_band_h,
            layer_id,
            content_epoch,
        })
    }

    // ── Composited layer SSOT ─────────────────────────────────

    /// Emit the one canonical command sequence for a [`Layer`]: restore the
    /// clean backdrop (and blur it, fresh or cached) when the layer is glass,
    /// then composite the content with its shadow/opacity/corner-radius effects.
    /// This is the SSOT every compositor element routes through instead of
    /// hand-rolling the blit→blur→composite recipe.
    ///
    /// The composite's own `backdrop_blur` is always 0 — the frosted blur is the
    /// explicit `BlurBackdrop` above, which keeps the layer's own content (text)
    /// sharp over the blurred backdrop rather than smearing it.
    ///
    /// `bounds` is the display extent `(width, height)`, used to clamp the restore
    /// halo (`LayerBackdrop::restore_halo_pad`) inside the framebuffer.
    pub fn composite_layer_full(
        &mut self,
        layer: &Layer,
        bounds: (u32, u32),
    ) -> Result<(), GfxError> {
        let rect =
            TileRect { x: layer.dst_x, y: layer.dst_y, width: layer.width, height: layer.height };
        if let Some(bd) = layer.backdrop {
            // Restore halo: grow the restore blit by `pad` on every side (clamped
            // to the display) so a soft shadow blends over a clean backdrop. The
            // blur + cache still cover only the layer rect.
            let pad = bd.restore_halo_pad;
            let hx = layer.dst_x.saturating_sub(pad);
            let hy = layer.dst_y.saturating_sub(pad);
            let hw = (layer.width + 2 * pad).min(bounds.0.saturating_sub(hx));
            let hh = (layer.height + 2 * pad).min(bounds.1.saturating_sub(hy));
            match bd.cache {
                BackdropCache::None => {
                    self.try_blit_surface(hx, hy + bd.retained_src_y_offset, hx, hy, hw, hh)?;
                    self.try_blur_backdrop(rect, bd.blur_radius, bd.saturation_percent)?;
                }
                BackdropCache::Write { cache_x, cache_row_abs, display_row_offset } => {
                    self.try_blit_surface(hx, hy + bd.retained_src_y_offset, hx, hy, hw, hh)?;
                    self.try_blur_backdrop(rect, bd.blur_radius, bd.saturation_percent)?;
                    self.try_blit_absolute(
                        layer.dst_x,
                        display_row_offset + layer.dst_y,
                        cache_x,
                        cache_row_abs,
                        layer.width,
                        layer.height,
                    )?;
                }
                BackdropCache::Read { cache_x, cache_row_abs, display_row_offset } => {
                    // The cache only repaints the layer rect; when there is a shadow
                    // halo, restore the surrounding pad from the retained plane first
                    // so the shadow blends over a clean backdrop.
                    if pad > 0 {
                        self.try_blit_surface(hx, hy + bd.retained_src_y_offset, hx, hy, hw, hh)?;
                    }
                    self.try_blit_absolute(
                        cache_x,
                        cache_row_abs,
                        layer.dst_x,
                        display_row_offset + layer.dst_y,
                        layer.width,
                        layer.height,
                    )?;
                }
            }
        }
        let (shadow_blur, shadow_offset_y, shadow_alpha) = match layer.shadow {
            Some(s) => (s.blur, s.offset_y, s.alpha),
            None => (0, 0, 0),
        };
        // Forward the frosted-blur radius. The gpud build-up RT compositor
        // (gl_scanout.rs `blur_rt_backdrop`, run from `composite_pending_rt_layers`)
        // GPU-blurs the wallpaper behind each glass layer's rect before
        // compositing it — the real frosted blur that reaches the virgl scanout.
        let backdrop_blur = layer.backdrop.map(|b| b.blur_radius).unwrap_or(0);
        self.try_composite_layer_tagged(
            layer.src_row_abs,
            layer.src_x,
            layer.width,
            layer.height,
            layer.dst_x,
            layer.dst_y,
            layer.opacity,
            layer.corner_radius,
            shadow_blur,
            shadow_offset_y,
            shadow_alpha,
            backdrop_blur,
            layer.scroll_id,
            layer.content_w,
            layer.content_h,
            layer.scroll_band_top_abs,
            layer.scroll_band_h,
            layer.layer_id,
            layer.content_epoch,
        )
    }

    // ── Backdrop blur ─────────────────────────────────────────

    /// Apply box blur + saturation to a framebuffer region.
    pub fn blur_backdrop(&mut self, rect: TileRect, radius: u32, saturation_percent: u32) {
        self.try_blur_backdrop(rect, radius, saturation_percent).expect("invalid blur");
    }

    pub fn try_blur_backdrop(
        &mut self,
        rect: TileRect,
        radius: u32,
        saturation_percent: u32,
    ) -> Result<(), GfxError> {
        if !self.active {
            return Err(GfxError::CommandRejected);
        }
        self.cmd.push_command(Command::BlurBackdrop { rect, radius, saturation_percent })
    }

    // ── Cursor ────────────────────────────────────────────────

    /// Blend the cursor bitmap onto the framebuffer.
    pub fn blend_cursor(&mut self, x: u32, y: u32, width: u32, height: u32) {
        self.try_blend_cursor(x, y, width, height).expect("invalid cursor blend");
    }

    pub fn try_blend_cursor(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), GfxError> {
        if !self.active {
            return Err(GfxError::CommandRejected);
        }
        self.cmd.push_command(Command::BlendCursor { x, y, width, height })
    }

    // ── Lifecycle ─────────────────────────────────────────────

    pub fn end_encoding(self) {
        let _ = self;
    }
}

impl Drop for RenderCommandEncoder<'_> {
    fn drop(&mut self) {
        self.active = false;
    }
}
