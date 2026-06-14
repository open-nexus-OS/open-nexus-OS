// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::command::buffer::{Command, CommandBuffer, RgbaColor};
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
        })
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
