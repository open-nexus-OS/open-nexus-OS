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
        self.bootstrap_splash_live = true;
        Ok(())
    }

    /// Create and present the branded boot splash (radial glow + wordmark) as
    /// the early 2D bootstrap scanout — the SAME image the GL splash shows, so
    /// the later scanout switch is visually seamless and the pulse animates
    /// from the very first frame (task #122). Fails (caller falls back to the
    /// text splash) when the wordmark asset was not rasterized.
    pub fn attach_bootstrap_splash_scanout(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(), GfxError> {
        if SPLASH_LOGO_W == 0 || SPLASH_LOGO_H == 0 {
            return Err(GfxError::Unsupported);
        }
        let resource = self.create_resource(width, height, PixelFormat::Bgra8888)?;
        let record = self.find_resource(resource).ok_or(GfxError::InvalidArgument)?;
        let pixel_len = width as usize * height as usize * 4;
        if pixel_len > record.backing_len {
            return Err(GfxError::ResourceExhausted);
        }
        let pixels =
            unsafe { core::slice::from_raw_parts_mut(record.backing_va as *mut u8, pixel_len) };
        compose_splash_region(pixels, width, height, 0, 0, width, height, 256);
        self.set_scanout_os(record)?;
        let full = Rect { x: 0, y: 0, width, height };
        self.transfer_to_host_os(record, full)?;
        self.flush_rect_os(record, full)?;
        self.scanout_resource = Some(resource);
        self.bootstrap_splash_live = true;
        Ok(())
    }

    /// Re-render the wordmark band at `factor_q8/256` brightness and present
    /// just that band — the boot-splash "breathe" during the 2D phase (before
    /// the GL scanout exists). The image is procedural (glow) + a static asset
    /// (wordmark), so the redraw needs no pristine copy; the glow is NOT scaled
    /// (see `compose_splash_region`), so the band has no seam against the
    /// static surround. No-op once windowd's framebuffer handoff replaced the
    /// bootstrap scanout.
    /// (Driven from the virgl frame-paced tick in service.rs; the 2D-only
    /// slice has no self-tick, hence the scoped allow.)
    #[cfg_attr(not(feature = "virgl"), allow(dead_code))]
    pub(crate) fn pulse_bootstrap_splash(&mut self, factor_q8: u32) -> Result<(), GfxError> {
        if !self.bootstrap_splash_live || SPLASH_LOGO_W == 0 || SPLASH_LOGO_H == 0 {
            return Ok(());
        }
        let Some(resource) = self.scanout_resource else {
            return Ok(());
        };
        // Proof (once): the 2D splash pulse actually animates in this boot.
        if !SPLASH_PULSE_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed) {
            let _ = nexus_abi::debug_println("gpud: splash pulse alive");
        }
        let record = self.find_resource(resource).ok_or(GfxError::InvalidArgument)?;
        let (width, height) = (record.width, record.height);
        let pixel_len = width as usize * height as usize * 4;
        if pixel_len > record.backing_len {
            return Err(GfxError::ResourceExhausted);
        }
        let pixels =
            unsafe { core::slice::from_raw_parts_mut(record.backing_va as *mut u8, pixel_len) };
        let lx0 = width.saturating_sub(SPLASH_LOGO_W) / 2;
        let ly0 = height.saturating_sub(SPLASH_LOGO_H) / 2;
        compose_splash_region(
            pixels,
            width,
            height,
            lx0,
            ly0,
            SPLASH_LOGO_W,
            SPLASH_LOGO_H,
            factor_q8,
        );
        let band = Rect { x: lx0, y: ly0, width: SPLASH_LOGO_W, height: SPLASH_LOGO_H };
        self.transfer_to_host_os(record, band)?;
        self.flush_rect_os(record, band)?;
        Ok(())
    }
}

// The branded wordmark, rasterized at build time (see gpud's build.rs). Owned
// HERE (the earliest phase that draws it); the GL splash imports it via the
// backend re-export — one copy in the binary, one compose implementation.
include!(concat!(env!("OUT_DIR"), "/splash_logo_dims.rs"));
/// Premultiplied BGRA wordmark pixels (`SPLASH_LOGO_W × SPLASH_LOGO_H`; 0×0
/// when rasterization was skipped — every composite becomes a no-op).
pub(crate) static SPLASH_LOGO_BGRA: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/splash_logo.bgra"));

/// Compose the boot-splash image into `dst` (BGRA, `fb_w`-pixel stride) for the
/// region `x0,y0,rw,rh`: the brand radial glow (center ~RGB(40,50,68) → edge
/// ~RGB(13,16,23)) with the wordmark premultiplied OVER it, the wordmark's
/// colour scaled by `logo_factor_q8/256` (256 = full). The glow is NOT scaled —
/// so the pulse band needs no margin and shows no seam against the static
/// surround; the wordmark visibly breathes against it. One implementation for
/// the 2D bootstrap attach, the 2D pulse band and the GL wallpaper seed.
pub(crate) fn compose_splash_region(
    dst: &mut [u8],
    fb_w: u32,
    fb_h: u32,
    x0: u32,
    y0: u32,
    rw: u32,
    rh: u32,
    logo_factor_q8: u32,
) {
    if fb_w == 0 || fb_h == 0 || dst.len() < fb_w as usize * fb_h as usize * 4 {
        return;
    }
    let f = logo_factor_q8.min(256);
    const C: [i32; 3] = [68, 50, 40]; // center B,G,R
    const E: [i32; 3] = [23, 16, 13]; // edge   B,G,R
    let cx = fb_w as i32 / 2;
    let cy = fb_h as i32 / 2;
    let max_d2 = (cx * cx + cy * cy).max(1) as u32;
    let lx0 = fb_w.saturating_sub(SPLASH_LOGO_W) / 2;
    let ly0 = fb_h.saturating_sub(SPLASH_LOGO_H) / 2;
    for y in y0..(y0 + rh).min(fb_h) {
        let dy = y as i32 - cy;
        for x in x0..(x0 + rw).min(fb_w) {
            let dx = x as i32 - cx;
            let d2 = (dx * dx + dy * dy) as u32;
            let t = (d2.saturating_mul(256) / max_d2).min(256) as i32;
            let off = (y as usize * fb_w as usize + x as usize) * 4;
            let mut px = [
                (C[0] + (E[0] - C[0]) * t / 256) as u32,
                (C[1] + (E[1] - C[1]) * t / 256) as u32,
                (C[2] + (E[2] - C[2]) * t / 256) as u32,
            ];
            if SPLASH_LOGO_W > 0
                && x >= lx0
                && x < lx0 + SPLASH_LOGO_W
                && y >= ly0
                && y < ly0 + SPLASH_LOGO_H
            {
                let src = (((y - ly0) * SPLASH_LOGO_W + (x - lx0)) * 4) as usize;
                let a = SPLASH_LOGO_BGRA[src + 3] as u32;
                if a != 0 {
                    // Premultiplied `src OVER dst`, wordmark colour scaled by f.
                    for c in 0..3 {
                        let s = SPLASH_LOGO_BGRA[src + c] as u32 * f / 256;
                        px[c] = (s + px[c] * (255 - a) / 255).min(255);
                    }
                }
            }
            dst[off] = px[0] as u8;
            dst[off + 1] = px[1] as u8;
            dst[off + 2] = px[2] as u8;
            dst[off + 3] = 255;
        }
    }
}

/// Latches once the first 2D splash-pulse frame is presented (one proof marker
/// per boot, no UART storm).
static SPLASH_PULSE_LOGGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Wall-clock anchor for the boot-splash pulse, latched on the first sample so
/// the 2D text phase and the GL glow phase share ONE continuous breathing curve
/// across the scanout switch (both render `f(now)` — cadence changes never bend
/// the curve, they only sample it).
static SPLASH_PULSE_ANCHOR_NS: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// One breathing cycle of the splash pulse.
const SPLASH_PULSE_PERIOD_NS: u64 = 1_200_000_000;

/// Brightness dip per step — one (1-cos)/2 cycle over 32 steps, max dip 56/256
/// (~22% dimming at the trough). A LUT keeps this integer-only (no float/libm
/// in the no_std service) — smooth enough at this amplitude.
const SPLASH_PULSE_DIP: [u8; 32] = [
    0, 1, 2, 5, 8, 12, 17, 23, 28, 33, 39, 44, 48, 51, 54, 55, 56, 55, 54, 51, 48, 44, 39, 33, 28,
    23, 17, 12, 8, 5, 2, 1,
];

/// Boot-splash brightness factor in q8 (256 = full brightness) at `now_ns`.
/// (Sampled by the virgl service tick and GL splash; unused in the 2D-only
/// slice, hence the scoped allow.)
#[cfg_attr(not(feature = "virgl"), allow(dead_code))]
pub(crate) fn splash_pulse_q8(now_ns: u64) -> u32 {
    let _ = SPLASH_PULSE_ANCHOR_NS.compare_exchange(
        0,
        now_ns.max(1),
        core::sync::atomic::Ordering::Relaxed,
        core::sync::atomic::Ordering::Relaxed,
    );
    let anchor = SPLASH_PULSE_ANCHOR_NS.load(core::sync::atomic::Ordering::Relaxed);
    let t = now_ns.saturating_sub(anchor) % SPLASH_PULSE_PERIOD_NS;
    let idx = ((t.saturating_mul(32)) / SPLASH_PULSE_PERIOD_NS) as usize % 32;
    256 - SPLASH_PULSE_DIP[idx] as u32
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
