// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CPU/VMO software rasterizer — thin adapters onto the canonical
//! [`nexus_gfx::raster`] primitives.
//!
//! Each function maps the framebuffer VMO (`*mut u8` + length) to a borrowed
//! [`Surface`] **once** and delegates the actual fill/blur/blit/blend to the
//! shared, `forbid(unsafe_code)` rasterizer in nexus-gfx — the single source of
//! truth that the host reference backend ([`nexus_gfx::backend::cpu_mock`]) also
//! runs. The only `unsafe` here is that one slice construction at the VMO
//! boundary; the per-frame blur/blit scratch is fixed stack memory, so this path
//! never allocates (the rule that keeps the bump-allocated service from OOMing).
//!
//! The per-pixel blend helpers ([`blend_pixel_vmo`], [`blend_premultiplied_vmo`])
//! keep the raw-pointer / `write_volatile` access the cursor overlay needs, but
//! delegate the blend math to the same `nexus_gfx::raster` cores.

#![cfg(all(feature = "os-lite", target_os = "none"))]

use core::ptr::{read_volatile, write_volatile};

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::command::buffer::RgbaColor;
use nexus_gfx::raster::{self, Surface};

/// Borrow the framebuffer VMO as a `fb_w`-wide BGRA surface.
///
/// # Safety
/// `fb` must point to `fb_len` valid, writable, properly aligned bytes that
/// outlive the returned surface, with no other live reference to that range.
#[inline]
unsafe fn surface<'a>(fb: *mut u8, fb_len: usize, fb_w: usize) -> Surface<'a> {
    Surface::new(core::slice::from_raw_parts_mut(fb, fb_len), fb_w as u32)
}

pub(crate) fn fill_rect_solid_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    color: [u8; 4],
) {
    let mut s = unsafe { surface(fb, fb_len, fb_w) };
    raster::fill_rect_solid(&mut s, x, y, w, h, color);
}

pub(crate) fn fill_sdf_rounded_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    color: RgbaColor,
) {
    let mut s = unsafe { surface(fb, fb_len, fb_w) };
    raster::fill_rounded_solid(&mut s, x, y, w, h, radius, color.as_array());
}

pub(crate) fn blur_backdrop_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    saturation_pct: u32,
) -> Result<(), GfxError> {
    // Stack scratch — worst case a full-width row (1280·4) and a full-plane
    // column (800·4). No per-frame heap traffic.
    let _ = saturation_pct; // historical 2D path does not boost saturation
    let mut scratch_row = [0u8; 5120];
    let mut scratch_col = [0u8; 3200];
    let mut s = unsafe { surface(fb, fb_len, fb_w) };
    raster::blur_box(&mut s, x, y, w, h, radius, &mut scratch_row, &mut scratch_col)
        .map_err(|_| GfxError::ResourceExhausted)
}

/// Separable gaussian blur — the virgl GPU path's CPU reference/fallback.
pub(crate) fn blur_backdrop_separable_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    saturation_pct: u32,
) -> Result<(), GfxError> {
    let _ = saturation_pct; // historical separable path does not boost saturation
    let mut scratch_row = [0u8; 5120];
    let mut scratch_col = [0u8; 3200];
    let mut s = unsafe { surface(fb, fb_len, fb_w) };
    raster::blur_gaussian(&mut s, x, y, w, h, radius, &mut scratch_row, &mut scratch_col)
        .map_err(|_| GfxError::ResourceExhausted)
}

pub(crate) fn blit_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    src_x: u32,
    src_y: u32,
    dst_x: u32,
    dst_y: u32,
    w: u32,
    h: u32,
) -> Result<(), GfxError> {
    let mut scratch_row = [0u8; 5120];
    let mut s = unsafe { surface(fb, fb_len, fb_w) };
    raster::blit_within(&mut s, src_x, src_y, dst_x, dst_y, w, h, &mut scratch_row)
        .map_err(|_| GfxError::ResourceExhausted)
}

/// Like `blit_vmo`, but ALPHA-BLENDS the source over the destination — the CPU
/// glass path compositing a translucent layer over a blurred backdrop.
#[allow(clippy::too_many_arguments)]
pub(crate) fn blit_blend_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    src_x: u32,
    src_y: u32,
    dst_x: u32,
    dst_y: u32,
    w: u32,
    h: u32,
) -> Result<(), GfxError> {
    let mut scratch_row = [0u8; 5120];
    let mut s = unsafe { surface(fb, fb_len, fb_w) };
    raster::blit_within_blend(&mut s, src_x, src_y, dst_x, dst_y, w, h, &mut scratch_row)
        .map_err(|_| GfxError::ResourceExhausted)
}

/// Composite a premultiplied-alpha BGRA pixel over the destination VMO pixel.
pub(crate) fn blend_premultiplied_vmo(fb: *mut u8, idx: usize, src: &[u8; 4]) {
    unsafe {
        let dst = [
            read_volatile(fb.add(idx)),
            read_volatile(fb.add(idx + 1)),
            read_volatile(fb.add(idx + 2)),
            read_volatile(fb.add(idx + 3)),
        ];
        let out = raster::blend_premultiplied_px(dst, *src);
        write_volatile(fb.add(idx), out[0]);
        write_volatile(fb.add(idx + 1), out[1]);
        write_volatile(fb.add(idx + 2), out[2]);
        write_volatile(fb.add(idx + 3), out[3]);
    }
}

/// Straight-alpha source-over of a BGRA pixel into the destination VMO pixel.
pub(crate) fn blend_pixel_vmo(fb: *mut u8, idx: usize, src: &[u8; 4]) {
    if src[3] == 0 {
        return;
    }
    unsafe {
        let dst = [
            read_volatile(fb.add(idx)),
            read_volatile(fb.add(idx + 1)),
            read_volatile(fb.add(idx + 2)),
            read_volatile(fb.add(idx + 3)),
        ];
        let out = raster::blend_over_px(dst, *src);
        write_volatile(fb.add(idx), out[0]);
        write_volatile(fb.add(idx + 1), out[1]);
        write_volatile(fb.add(idx + 2), out[2]);
        write_volatile(fb.add(idx + 3), out[3]);
    }
}
