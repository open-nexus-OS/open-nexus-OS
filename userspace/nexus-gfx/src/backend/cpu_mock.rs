// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CPU mock backend — deterministic reference implementation for GPU command execution.
//! Executes all CommandBuffer commands via the canonical [`crate::raster`] software
//! rasterizer (the same primitives the live GPU driver's CPU/VMO fallback runs), so
//! this golden reference and the production path share one implementation.

use crate::backend::error::GfxError;
use crate::backend::traits::GfxBackend;
use crate::backend::types::{Rect, ResourceId};
use crate::command::buffer::{Command, CommittedBuffer, RgbaColor};
use crate::core::fence::Fence;
use crate::core::types::PixelFormat;
use crate::raster::{self, Surface};
use alloc::{vec, vec::Vec};

#[allow(dead_code)]
struct CpuResource {
    width: u32,
    height: u32,
    format: PixelFormat,
    data: Vec<u8>,
}

/// Software rasterizer implementing GfxBackend.
/// Owns its framebuffer and executes all Command types deterministically.
pub struct CpuMockBackend {
    framebuffer: Vec<u8>,
    width: u32,
    height: u32,
    /// Source surface for BlitSurface commands (wallpaper, icons).
    source_surface: Vec<u8>,
    source_width: u32,
    source_height: u32,
    /// Cursor bitmap for BlendCursor commands.
    cursor_bitmap: Vec<u8>,
    cursor_width: u32,
    cursor_height: u32,
    resources: Vec<CpuResource>,
    next_id: u32,
    /// Fragment uniform storage (SetFragmentBytes).
    fragment_data: [u8; 64],
}

impl CpuMockBackend {
    #[must_use]
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            framebuffer: vec![0u8; w as usize * h as usize * 4],
            width: w,
            height: h,
            source_surface: Vec::new(),
            source_width: 0,
            source_height: 0,
            cursor_bitmap: Vec::new(),
            cursor_width: 0,
            cursor_height: 0,
            resources: vec![],
            next_id: 1,
            fragment_data: [0u8; 64],
        }
    }

    /// Set the source surface for BlitSurface commands.
    pub fn set_source_surface(&mut self, pixels: &[u8], w: u32, h: u32) {
        self.source_surface.clear();
        self.source_surface.extend_from_slice(pixels);
        self.source_width = w;
        self.source_height = h;
    }

    /// Set the cursor bitmap for BlendCursor commands.
    pub fn set_cursor(&mut self, pixels: &[u8], w: u32, h: u32) {
        self.cursor_bitmap.clear();
        self.cursor_bitmap.extend_from_slice(pixels);
        self.cursor_width = w;
        self.cursor_height = h;
    }

    #[must_use]
    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    // ── Command execution ────────────────────────────────────

    fn execute(&mut self, cmds: &[Command]) -> Result<(), GfxError> {
        for cmd in cmds {
            match cmd {
                Command::SetFragmentBytes { offset, data } => {
                    let end = offset.saturating_add(data.len());
                    if end > self.fragment_data.len() {
                        return Err(GfxError::CommandRejected);
                    }
                    self.fragment_data[*offset..end].copy_from_slice(data);
                }
                Command::DrawTiles { tiles, color } => {
                    let c = color.as_array();
                    for t in tiles {
                        self.fill_rect_solid(t.x, t.y, t.width, t.height, c);
                    }
                }
                Command::BlitSurface { src_x, src_y, dst_x, dst_y, width, height } => {
                    self.blit(*src_x, *src_y, *dst_x, *dst_y, *width, *height)?;
                }
                Command::FillSdfRoundedRect { rect, radius, color } => {
                    self.fill_sdf_rounded(rect.x, rect.y, rect.width, rect.height, *radius, *color);
                }
                Command::BlurBackdrop { rect, radius, saturation_percent } => {
                    self.blur_backdrop(
                        rect.x,
                        rect.y,
                        rect.width,
                        rect.height,
                        *radius,
                        *saturation_percent,
                    )?;
                }
                Command::BlendCursor { x, y, width, height } => {
                    self.blend_cursor(*x, *y, *width, *height);
                }
                Command::BlitAbsolute { src_x, src_y_abs, dst_x, dst_y_abs, width, height } => {
                    self.blit(*src_x, *src_y_abs, *dst_x, *dst_y_abs, *width, *height)?;
                }
                Command::FillSdfGradient { rect, radius, color_top, color_bottom } => {
                    self.fill_sdf_gradient(
                        rect.x,
                        rect.y,
                        rect.width,
                        rect.height,
                        *radius,
                        *color_top,
                        *color_bottom,
                    );
                }
                Command::DropShadow { rect, radius, blur, offset_x, offset_y, color } => {
                    self.drop_shadow(
                        rect.x,
                        rect.y,
                        rect.width,
                        rect.height,
                        *radius,
                        *blur,
                        *offset_x,
                        *offset_y,
                        *color,
                    );
                }
                Command::CompositeLayer { .. } => {
                    // GPU-only op: composites a content texture into the GL
                    // scanout RT with per-layer effects. The CPU mock has no
                    // scanout-RT/atlas separation, so it is a no-op here (the
                    // real backend executes it; the 2D path bakes layers on the
                    // CPU and never emits this command).
                }
            }
        }
        Ok(())
    }

    // ── Rendering primitives (thin wrappers over the canonical rasterizer) ──

    fn fill_rect_solid(&mut self, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
        let width = self.width;
        let mut s = Surface::new(&mut self.framebuffer, width);
        raster::fill_rect_solid(&mut s, x, y, w, h, color);
    }

    fn fill_sdf_rounded(&mut self, x: u32, y: u32, w: u32, h: u32, radius: u32, color: RgbaColor) {
        let width = self.width;
        let mut s = Surface::new(&mut self.framebuffer, width);
        raster::fill_rounded_aa(&mut s, x, y, w, h, radius, color.as_array());
    }

    #[allow(clippy::too_many_arguments)]
    fn fill_sdf_gradient(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        radius: u32,
        top: RgbaColor,
        bottom: RgbaColor,
    ) {
        let width = self.width;
        let mut s = Surface::new(&mut self.framebuffer, width);
        raster::fill_gradient_aa(&mut s, x, y, w, h, radius, top.as_array(), bottom.as_array());
    }

    #[allow(clippy::too_many_arguments)]
    fn drop_shadow(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        radius: u32,
        blur: u32,
        offset_x: i32,
        offset_y: i32,
        color: RgbaColor,
    ) {
        let width = self.width;
        let height = self.height;
        let mut s = Surface::new(&mut self.framebuffer, width);
        raster::drop_shadow(
            &mut s,
            x,
            y,
            w,
            h,
            radius,
            blur,
            offset_x,
            offset_y,
            color.as_array(),
            0,
            height,
        );
    }

    fn blur_backdrop(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        radius: u32,
        saturation_pct: u32,
    ) -> Result<(), GfxError> {
        let width = self.width;
        let mut scratch_row = vec![0u8; self.width as usize * 4];
        let mut scratch_col = vec![0u8; self.height as usize * 4];
        let mut s = Surface::new(&mut self.framebuffer, width);
        raster::blur_box(&mut s, x, y, w, h, radius, &mut scratch_row, &mut scratch_col)
            .map_err(|_| GfxError::ResourceExhausted)?;
        raster::saturate(&mut s, x, y, w, h, saturation_pct);
        Ok(())
    }

    fn blit(&mut self, sx: u32, sy: u32, dx: u32, dy: u32, w: u32, h: u32) -> Result<(), GfxError> {
        if self.source_surface.is_empty() || self.source_width == 0 {
            return Ok(()); // no source → skip silently
        }
        let width = self.width;
        let src_w = self.source_width;
        let src_h = self.source_height;
        let src = &self.source_surface;
        let mut s = Surface::new(&mut self.framebuffer, width);
        raster::blit_from(&mut s, src, src_w, src_h, sx, sy, dx, dy, w, h);
        Ok(())
    }

    fn blend_cursor(&mut self, x: u32, y: u32, w: u32, h: u32) {
        if self.cursor_bitmap.is_empty() {
            return;
        }
        let fw = self.width;
        let fh = self.height;
        let cw = self.cursor_width;
        let sprite = &self.cursor_bitmap;
        let fb = &mut self.framebuffer;
        for row in 0..h {
            let dst_y = y.saturating_add(row);
            if dst_y >= fh {
                break;
            }
            for col in 0..w {
                let dst_x = x.saturating_add(col);
                if dst_x >= fw {
                    break;
                }
                let src_i = (row as usize * cw as usize + col as usize) * 4;
                if src_i + 4 > sprite.len() {
                    continue;
                }
                let a = sprite[src_i + 3];
                if a == 0 {
                    continue;
                }
                let idx = (dst_y as usize * fw as usize + dst_x as usize) * 4;
                raster::blend_over(
                    fb,
                    idx,
                    &[sprite[src_i], sprite[src_i + 1], sprite[src_i + 2], a],
                );
            }
        }
    }
}

// ── GfxBackend impl ──────────────────────────────────────────

impl GfxBackend for CpuMockBackend {
    fn submit(&mut self, cmd: CommittedBuffer) -> Result<Fence, GfxError> {
        cmd.validate().map_err(GfxError::from)?;
        // Phase 6d: honest fence lifecycle.
        let mut fence = Fence::new_unsignaled();
        self.execute(cmd.commands())?;
        fence.signal();
        Ok(fence)
    }

    fn create_resource(
        &mut self,
        w: u32,
        h: u32,
        fmt: PixelFormat,
    ) -> Result<ResourceId, GfxError> {
        if w == 0 || h == 0 {
            return Err(GfxError::InvalidArgument);
        }
        let id = ResourceId(self.next_id);
        self.next_id += 1;
        self.resources.push(CpuResource {
            width: w,
            height: h,
            format: fmt,
            data: vec![0u8; w as usize * h as usize * 4],
        });
        Ok(id)
    }

    fn transfer_to_host(&mut self, r: ResourceId, rect: Rect) -> Result<(), GfxError> {
        let Some(resource) = self.resources.get((r.0.saturating_sub(1)) as usize) else {
            return Err(GfxError::InvalidArgument);
        };
        let end_x = rect.x.checked_add(rect.width).ok_or(GfxError::InvalidArgument)?;
        let end_y = rect.y.checked_add(rect.height).ok_or(GfxError::InvalidArgument)?;
        if rect.width == 0 || rect.height == 0 || end_x > resource.width || end_y > resource.height
        {
            return Err(GfxError::InvalidArgument);
        }
        Ok(())
    }

    fn set_scanout(&mut self, r: ResourceId) -> Result<(), GfxError> {
        if self.resources.get((r.0.saturating_sub(1)) as usize).is_none() {
            return Err(GfxError::InvalidArgument);
        }
        Ok(())
    }

    fn move_cursor(&mut self, _x: i32, _y: i32) -> Result<(), GfxError> {
        Ok(())
    }
}
