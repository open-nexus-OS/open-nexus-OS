// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CPU mock backend — deterministic reference implementation for GPU command execution.
//! Executes all CommandBuffer commands as a software rasterizer.
//! Used as the golden reference for backend correctness proofs.

use crate::backend::error::GfxError;
use crate::backend::traits::GfxBackend;
use crate::backend::types::{Rect, ResourceId};
use crate::command::buffer::{Command, CommittedBuffer, RgbaColor};
use crate::core::fence::Fence;
use crate::core::types::PixelFormat;
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
                Command::DrawTiles { tiles } => {
                    // Derive tile color from fragment uniform data (animation state).
                    let color = self.tile_color_from_fragment();
                    for t in tiles {
                        self.fill_rect_solid(t.x, t.y, t.width, t.height, color);
                    }
                }
                Command::BlitSurface { src_x, src_y, dst_x, dst_y, width, height } => {
                    self.blit(*src_x, *src_y, *dst_x, *dst_y, *width, *height)?;
                }
                Command::FillSdfRoundedRect { rect, radius, color } => {
                    self.fill_sdf_rounded(rect.x, rect.y, rect.width, rect.height, *radius, *color);
                }
                Command::BlurBackdrop { rect, radius, saturation_percent } => {
                    self.blur_backdrop(rect.x, rect.y, rect.width, rect.height, *radius, *saturation_percent)?;
                }
                Command::BlendCursor { x, y, width, height } => {
                    self.blend_cursor(*x, *y, *width, *height);
                }
            }
        }
        Ok(())
    }

    // ── Rendering primitives ─────────────────────────────────

    fn tile_color_from_fragment(&self) -> [u8; 4] {
        let sidebar_opacity = f32::from_le_bytes([
            self.fragment_data[12], self.fragment_data[13],
            self.fragment_data[14], self.fragment_data[15],
        ]);
        let alpha = (sidebar_opacity.clamp(0.0, 1.0) * 192.0) as u8;
        if alpha > 0 { [200, 220, 255, alpha] } else { [0, 0, 0, 0] }
    }

    fn fill_rect_solid(&mut self, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
        let fw = self.width as usize;
        let end_x = x.saturating_add(w).min(self.width);
        let end_y = y.saturating_add(h).min(self.height);
        for py in y..end_y {
            let row = py as usize * fw;
            for px in x..end_x {
                let i = (row + px as usize) * 4;
                if i + 4 <= self.framebuffer.len() {
                    self.framebuffer[i..i + 4].copy_from_slice(&color);
                }
            }
        }
    }

    fn blit(&mut self, sx: u32, sy: u32, dx: u32, dy: u32, w: u32, h: u32) -> Result<(), GfxError> {
        if self.source_surface.is_empty() || self.source_width == 0 {
            return Ok(()); // no source → skip silently
        }
        let src_stride = self.source_width as usize * 4;
        let dst_stride = self.width as usize * 4;
        for row in 0..h.min(self.height.saturating_sub(dy)) {
            let src_y = sy.saturating_add(row);
            let dst_y = dy.saturating_add(row);
            if src_y >= self.source_height || dst_y >= self.height {
                break;
            }
            let src_off = src_y as usize * src_stride + sx as usize * 4;
            let dst_off = dst_y as usize * dst_stride + dx as usize * 4;
            let copy_len = (w as usize * 4).min(self.framebuffer.len().saturating_sub(dst_off));
            let src_end = src_off.saturating_add(copy_len);
            if src_end <= self.source_surface.len() && dst_off + copy_len <= self.framebuffer.len() {
                self.framebuffer[dst_off..dst_off + copy_len]
                    .copy_from_slice(&self.source_surface[src_off..src_end]);
            }
        }
        Ok(())
    }

    fn fill_sdf_rounded(&mut self, x: u32, y: u32, w: u32, h: u32, radius: u32, color: RgbaColor) {
        let rgba = color.as_array();
        if rgba[3] == 0 { return; }
        let fw = self.width as usize;
        let end_x = x.saturating_add(w).min(self.width);
        let end_y = y.saturating_add(h).min(self.height);
        let r = radius.min(w / 2).min(h / 2) as i32;
        let cx = x as i32 + r;
        let cy = y as i32 + r;
        let cx2 = x as i32 + w as i32 - r - 1;
        let cy2 = y as i32 + h as i32 - r - 1;
        for py in y..end_y {
            let row = py as usize * fw;
            for px in x..end_x {
                let i = (row + px as usize) * 4;
                if i + 4 > self.framebuffer.len() { continue; }
                let inside = if r <= 0 {
                    true
                } else {
                    let px = px as i32;
                    let py = py as i32;
                    let d = if px <= cx && py <= cy {
                        corner_dist(px, py, cx, cy, r)
                    } else if px >= cx2 && py <= cy {
                        corner_dist(px, py, cx2, cy, r)
                    } else if px <= cx && py >= cy2 {
                        corner_dist(px, py, cx, cy2, r)
                    } else if px >= cx2 && py >= cy2 {
                        corner_dist(px, py, cx2, cy2, r)
                    } else {
                        0
                    };
                    d <= 0
                };
                if inside {
                    blend_pixel(&mut self.framebuffer[i..i + 4], &rgba);
                }
            }
        }
    }

    fn blur_backdrop(&mut self, x: u32, y: u32, w: u32, h: u32, radius: u32, saturation_pct: u32) -> Result<(), GfxError> {
        if radius == 0 {
            return Ok(());
        }
        let fw = self.width as usize;
        let end_x = x.saturating_add(w).min(self.width);
        let end_y = y.saturating_add(h).min(self.height);
        let r = radius as usize;

        // Horizontal pass (in-place with scratch row)
        let mut scratch = vec![0u8; self.width as usize * 4];
        for py in y..end_y {
            let _row_off = py as usize * fw;
            let row_len = (end_x - x) as usize * 4;
            let row_start = x as usize * 4;
            if row_start + row_len > self.framebuffer.len() { continue; }
            scratch[..row_len].copy_from_slice(&self.framebuffer[row_start..row_start + row_len]);

            let pixels = row_len / 4;
            let mut sums = [0u64; 4];
            let mut left = 0usize;
            let mut right = r.min(pixels.saturating_sub(1));
            for j in left..=right {
                let bi = j * 4;
                for c in 0..4 { sums[c] += scratch[bi + c] as u64; }
            }
            for i in 0..pixels {
                let count = (right - left + 1) as u64;
                let di = row_start + i * 4;
                for c in 0..4 {
                    self.framebuffer[di + c] = (sums[c] / count.max(1)).min(255) as u8;
                }
                if i + 1 < pixels {
                    let next_left = (i + 1).saturating_sub(r);
                    if next_left > left {
                        let bi = left * 4;
                        for c in 0..4 { sums[c] = sums[c].saturating_sub(scratch[bi + c] as u64); }
                        left = next_left;
                    }
                    let next_right = (i + 1 + r).min(pixels.saturating_sub(1));
                    if next_right > right {
                        right = next_right;
                        let bi = right * 4;
                        for c in 0..4 { sums[c] += scratch[bi + c] as u64; }
                    }
                }
            }
        }

        // Vertical pass
        let mut col_buf = vec![0u8; (end_y - y) as usize * 4];
        for px in x..end_x {
            let col_off = px as usize * 4;
            let col_h = (end_y - y) as usize;
            for row_i in 0..col_h {
                let src = (y as usize + row_i) * fw + col_off;
                col_buf[row_i * 4..row_i * 4 + 4]
                    .copy_from_slice(&self.framebuffer[src..src + 4]);
            }
            let mut sums = [0u64; 4];
            let mut top = 0usize;
            let mut bot = r.min(col_h.saturating_sub(1));
            for j in top..=bot {
                let bi = j * 4;
                for c in 0..4 { sums[c] += col_buf[bi + c] as u64; }
            }
            for i in 0..col_h {
                let count = (bot - top + 1) as u64;
                let dst = (y as usize + i) * fw + col_off;
                for c in 0..4 {
                    self.framebuffer[dst + c] = (sums[c] / count.max(1)).min(255) as u8;
                }
                if i + 1 < col_h {
                    let next_top = (i + 1).saturating_sub(r);
                    if next_top > top {
                        let bi = top * 4;
                        for c in 0..4 { sums[c] = sums[c].saturating_sub(col_buf[bi + c] as u64); }
                        top = next_top;
                    }
                    let next_bot = (i + 1 + r).min(col_h.saturating_sub(1));
                    if next_bot > bot {
                        bot = next_bot;
                        let bi = bot * 4;
                        for c in 0..4 { sums[c] += col_buf[bi + c] as u64; }
                    }
                }
            }
        }

        // Saturation boost: saturate_bgra_segment
        if saturation_pct != 0 && saturation_pct != 100 {
            let factor = saturation_pct as f32 / 100.0;
            for py in y..end_y {
                let row_off = py as usize * fw + x as usize * 4;
                let row_len = (end_x - x) as usize * 4;
                for off in (row_off..row_off + row_len).step_by(4) {
                    if off + 4 > self.framebuffer.len() { continue; }
                    let b = self.framebuffer[off] as f32;
                    let g = self.framebuffer[off + 1] as f32;
                    let r = self.framebuffer[off + 2] as f32;
                    let gray = 0.299 * r + 0.587 * g + 0.114 * b;
                    self.framebuffer[off] = (gray + (b - gray) * factor).clamp(0.0, 255.0) as u8;
                    self.framebuffer[off + 1] = (gray + (g - gray) * factor).clamp(0.0, 255.0) as u8;
                    self.framebuffer[off + 2] = (gray + (r - gray) * factor).clamp(0.0, 255.0) as u8;
                }
            }
        }
        Ok(())
    }

    fn blend_cursor(&mut self, x: u32, y: u32, w: u32, h: u32) {
        if self.cursor_bitmap.is_empty() { return; }
        let fw = self.width as usize;
        for row in 0..h {
            let dst_y = y.saturating_add(row);
            if dst_y >= self.height { break; }
            for col in 0..w {
                let dst_x = x.saturating_add(col);
                if dst_x >= self.width { break; }
                let src_i = (row as usize * self.cursor_width as usize + col as usize) * 4;
                if src_i + 4 > self.cursor_bitmap.len() { continue; }
                let alpha = self.cursor_bitmap[src_i + 3] as u16;
                if alpha == 0 { continue; }
                let dst_i = (dst_y as usize * fw + dst_x as usize) * 4;
                if dst_i + 4 > self.framebuffer.len() { continue; }
                let inv = 255u16.saturating_sub(alpha);
                for c in 0..3 {
                    self.framebuffer[dst_i + c] = ((alpha * self.cursor_bitmap[src_i + c] as u16
                        + inv * self.framebuffer[dst_i + c] as u16) / 255) as u8;
                }
            }
        }
    }
}

// ── GfxBackend impl ──────────────────────────────────────────

impl GfxBackend for CpuMockBackend {
    fn submit(&mut self, cmd: CommittedBuffer) -> Result<Fence, GfxError> {
        cmd.validate().map_err(GfxError::from)?;
        self.execute(cmd.commands())?;
        Ok(Fence::new_signaled())
    }

    fn create_resource(&mut self, w: u32, h: u32, fmt: PixelFormat) -> Result<ResourceId, GfxError> {
        if w == 0 || h == 0 { return Err(GfxError::InvalidArgument); }
        let id = ResourceId(self.next_id);
        self.next_id += 1;
        self.resources.push(CpuResource {
            width: w, height: h, format: fmt,
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
        if rect.width == 0 || rect.height == 0 || end_x > resource.width || end_y > resource.height {
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

// ── Helpers ──────────────────────────────────────────────────

fn corner_dist(px: i32, py: i32, cx: i32, cy: i32, r: i32) -> i32 {
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy - r * r
}

fn blend_pixel(dst: &mut [u8], src: &[u8; 4]) {
    let alpha = src[3] as u32;
    if alpha == 0 { return; }
    if alpha == 255 {
        dst.copy_from_slice(src);
        return;
    }
    let inv = 255 - alpha;
    for c in 0..3 {
        dst[c] = ((alpha * src[c] as u32 + inv * dst[c] as u32) / 255) as u8;
    }
    dst[3] = dst[3].saturating_add(alpha as u8);
}
