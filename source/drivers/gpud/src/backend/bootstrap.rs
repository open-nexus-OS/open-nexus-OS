// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Early bootstrap scanout frames: solid-color / BGRA / scaled-BGRA images and
//! the centered boot text, presented before windowd hands over its composed
//! framebuffer VMO. Includes the tiny 5×7 bitmap font used for the boot text.

#![cfg(all(feature = "os-lite", target_os = "none"))]

use super::VirtioGpuBackend;
use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::traits::GfxBackend; // for the trait method `create_resource`
use nexus_gfx::backend::types::Rect;
use nexus_gfx::core::types::PixelFormat;

impl VirtioGpuBackend {
    /// Create and present a static solid-color scanout as an early bootstrap
    /// frame before windowd hands over its composed framebuffer VMO.
    pub fn attach_bootstrap_solid_scanout(
        &mut self,
        width: u32,
        height: u32,
        bgra: [u8; 4],
    ) -> Result<(), GfxError> {
        let resource = self.create_resource(width, height, PixelFormat::Bgra8888)?;
        let record = self.find_resource(resource).ok_or(GfxError::InvalidArgument)?;
        let pixel_len = width as usize * height as usize * 4;
        if pixel_len > record.backing_len {
            return Err(GfxError::ResourceExhausted);
        }
        let pixels =
            unsafe { core::slice::from_raw_parts_mut(record.backing_va as *mut u8, pixel_len) };
        for chunk in pixels.chunks_exact_mut(4) {
            chunk.copy_from_slice(&bgra);
        }
        self.set_scanout_os(record)?;
        let full = Rect { x: 0, y: 0, width, height };
        self.transfer_to_host_os(record, full)?;
        self.flush_rect_os(record, full)?;
        self.scanout_resource = Some(resource);
        Ok(())
    }

    /// Create and present a static BGRA scanout frame as early bootstrap.
    /// `pixels` must be exactly `width * height * 4` bytes.
    pub fn attach_bootstrap_bgra_scanout(
        &mut self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<(), GfxError> {
        let expected_len = width as usize * height as usize * 4;
        if pixels.len() != expected_len {
            return Err(GfxError::InvalidArgument);
        }
        let resource = self.create_resource(width, height, PixelFormat::Bgra8888)?;
        let record = self.find_resource(resource).ok_or(GfxError::InvalidArgument)?;
        if expected_len > record.backing_len {
            return Err(GfxError::ResourceExhausted);
        }
        let dst =
            unsafe { core::slice::from_raw_parts_mut(record.backing_va as *mut u8, expected_len) };
        dst.copy_from_slice(pixels);
        self.set_scanout_os(record)?;
        let full = Rect { x: 0, y: 0, width, height };
        self.transfer_to_host_os(record, full)?;
        self.flush_rect_os(record, full)?;
        self.scanout_resource = Some(resource);
        Ok(())
    }

    /// Create and present a static BGRA scanout from a smaller source image.
    /// Source pixels are nearest-neighbor upscaled to `(width,height)`.
    pub fn attach_bootstrap_scaled_bgra_scanout(
        &mut self,
        width: u32,
        height: u32,
        source_width: u32,
        source_height: u32,
        source_pixels: &[u8],
    ) -> Result<(), GfxError> {
        if source_width == 0 || source_height == 0 || width == 0 || height == 0 {
            return Err(GfxError::InvalidArgument);
        }
        let source_len = source_width as usize * source_height as usize * 4;
        if source_pixels.len() != source_len {
            return Err(GfxError::InvalidArgument);
        }
        let resource = self.create_resource(width, height, PixelFormat::Bgra8888)?;
        let record = self.find_resource(resource).ok_or(GfxError::InvalidArgument)?;
        let dst_len = width as usize * height as usize * 4;
        if dst_len > record.backing_len {
            return Err(GfxError::ResourceExhausted);
        }
        let dst = unsafe { core::slice::from_raw_parts_mut(record.backing_va as *mut u8, dst_len) };

        let src_w = source_width as usize;
        let src_h = source_height as usize;
        let out_w = width as usize;
        let out_h = height as usize;
        for y in 0..out_h {
            let src_y = y * src_h / out_h;
            for x in 0..out_w {
                let src_x = x * src_w / out_w;
                let src_idx = (src_y * src_w + src_x) * 4;
                let dst_idx = (y * out_w + x) * 4;
                dst[dst_idx..dst_idx + 4].copy_from_slice(&source_pixels[src_idx..src_idx + 4]);
            }
        }

        self.set_scanout_os(record)?;
        let full = Rect { x: 0, y: 0, width, height };
        self.transfer_to_host_os(record, full)?;
        self.flush_rect_os(record, full)?;
        self.scanout_resource = Some(resource);
        Ok(())
    }

    /// Create and present a black bootstrap scanout with centered text.
    pub fn attach_bootstrap_text_scanout(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(), GfxError> {
        let resource = self.create_resource(width, height, PixelFormat::Bgra8888)?;
        let record = self.find_resource(resource).ok_or(GfxError::InvalidArgument)?;
        let pixel_len = width as usize * height as usize * 4;
        if pixel_len > record.backing_len {
            return Err(GfxError::ResourceExhausted);
        }
        let pixels =
            unsafe { core::slice::from_raw_parts_mut(record.backing_va as *mut u8, pixel_len) };
        for chunk in pixels.chunks_exact_mut(4) {
            chunk.copy_from_slice(&[0, 0, 0, 255]);
        }

        draw_centered_bootstrap_line(
            pixels,
            width,
            height,
            (height as i32 / 2) - 80,
            "open nexus OS",
            12,
            [240, 240, 240, 255],
        );
        draw_centered_bootstrap_line(
            pixels,
            width,
            height,
            (height as i32 / 2) + 20,
            "One OS. Many Devices.",
            6,
            [190, 190, 190, 255],
        );
        draw_centered_bootstrap_line(
            pixels,
            width,
            height,
            (height as i32) - 70,
            "Powered by Risc-V",
            4,
            [150, 150, 150, 255],
        );

        self.set_scanout_os(record)?;
        let full = Rect { x: 0, y: 0, width, height };
        self.transfer_to_host_os(record, full)?;
        self.flush_rect_os(record, full)?;
        self.scanout_resource = Some(resource);
        Ok(())
    }
}

const BOOT_FONT_W: i32 = 5;
const BOOT_FONT_SPACING: i32 = 1;

fn draw_centered_bootstrap_line(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    top_y: i32,
    text: &str,
    scale: u32,
    color: [u8; 4],
) {
    if scale == 0 {
        return;
    }
    let scale_i = scale as i32;
    let text_w = bootstrap_text_width(text, scale_i);
    let start_x = (width as i32 - text_w) / 2;
    draw_bootstrap_text(pixels, width, height, start_x, top_y, text, scale_i, color);
}

fn bootstrap_text_width(text: &str, scale: i32) -> i32 {
    let count = text.chars().count() as i32;
    if count <= 0 {
        return 0;
    }
    count * (BOOT_FONT_W + BOOT_FONT_SPACING) * scale - BOOT_FONT_SPACING * scale
}

fn draw_bootstrap_text(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    text: &str,
    scale: i32,
    color: [u8; 4],
) {
    let mut pen_x = x;
    for ch in text.chars() {
        draw_bootstrap_char(pixels, width, height, pen_x, y, ch, scale, color);
        pen_x += (BOOT_FONT_W + BOOT_FONT_SPACING) * scale;
    }
}

fn draw_bootstrap_char(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    ch: char,
    scale: i32,
    color: [u8; 4],
) {
    let glyph = bootstrap_glyph(ch);
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..BOOT_FONT_W {
            let mask = 1u8 << (BOOT_FONT_W - 1 - col);
            if bits & mask == 0 {
                continue;
            }
            let px = x + col * scale;
            let py = y + row as i32 * scale;
            for dy in 0..scale {
                for dx in 0..scale {
                    put_bootstrap_pixel(pixels, width, height, px + dx, py + dy, color);
                }
            }
        }
    }
}

fn put_bootstrap_pixel(pixels: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let idx = ((y as usize * width as usize) + x as usize) * 4;
    if idx + 4 <= pixels.len() {
        pixels[idx..idx + 4].copy_from_slice(&color);
    }
}

fn bootstrap_glyph(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0F, 0x10, 0x10, 0x10, 0x10, 0x10, 0x0F],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0F, 0x10, 0x10, 0x17, 0x11, 0x11, 0x0E],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1F],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        _ => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    }
}
