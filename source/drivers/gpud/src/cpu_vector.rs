// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: CPU fallback for the GPU vector pipeline (FillSdfGradient /
//! DropShadow) — used on non-virgl backends (mmio 2D path) and when the virgl
//! submit fails. Thin adapters onto the canonical [`nexus_gfx::raster`]
//! primitives, so the fallback shares the live compositor's anti-aliased SDF
//! rasterization (one math SSOT, RFC-0067 P5).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0063-ui-v5b-scene-graph-gpu-pipeline-virtual-list-theme-contract.md

#![cfg(all(feature = "os-lite", target_os = "none"))]

use nexus_gfx::command::buffer::RgbaColor;
use nexus_gfx::raster::{self, Surface};

/// Fill an SDF rounded rect with a vertical linear gradient (top → bottom).
/// `y` is the absolute fb row (display offset already applied by the caller).
#[allow(clippy::too_many_arguments)]
pub(crate) fn fill_sdf_gradient_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    top: RgbaColor,
    bottom: RgbaColor,
) {
    let buf = unsafe { core::slice::from_raw_parts_mut(fb, fb_len) };
    let mut s = Surface::new(buf, fb_w as u32);
    raster::fill_gradient_aa(&mut s, x, y, w, h, radius, top.as_array(), bottom.as_array());
}

/// Soft drop shadow for a rounded rect: quadratic SDF falloff over `blur` px.
/// `(x, y, w, h)` is the casting shape with `y` absolute; the painted band is
/// clamped to the display plane (`display_row .. display_row + display_h`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn drop_shadow_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    blur: u32,
    offset_x: i32,
    offset_y: i32,
    color: RgbaColor,
    display_row: u32,
    display_h: u32,
) {
    let buf = unsafe { core::slice::from_raw_parts_mut(fb, fb_len) };
    let mut s = Surface::new(buf, fb_w as u32);
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
        display_row,
        display_row.saturating_add(display_h),
    );
}
