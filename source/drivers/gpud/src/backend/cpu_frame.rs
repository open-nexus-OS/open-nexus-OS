// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The CPU executor of windowd's present commands — ONE implementation for every
//! display that composites on the CPU: the virtio GPU's 2D path (and the per-command fallbacks
//! of its virgl path) on QEMU, and the board's display controller (TASK-0251 P2a step 2). A
//! command lands in the shared framebuffer through `nexus_gfx::raster` (`raster.rs`' adapters):
//! screen-relative commands at the display plane's row, absolute ones where they say. What the
//! commands read besides the framebuffer lives here too: windowd's fragment uniforms and the
//! software cursor (its sprite, its hotspot and the shape cache windowd fills once).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the sprite store below (host, `test_reject_*`); the raster primitives in
//!   nexus-gfx (host); every QEMU lane on the 2D path (`gpud: chain G3 exec ok`, the reveal);
//!   the board ladder (`windowd: desktop revealed`)

use alloc::vec::Vec;

use nexus_gfx::backend::error::GfxError;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use nexus_gfx::command::buffer::{Command, RgbaColor};

#[cfg(all(feature = "os-lite", target_os = "none"))]
use super::cursor::blend_cursor_vmo;
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
use super::raster::blur_backdrop_separable_vmo;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use super::raster::{
    blit_blend_vmo, blit_vmo, blur_backdrop_vmo, fill_rect_solid_vmo, fill_sdf_rounded_vmo,
};

/// One cached cursor shape: `(premultiplied BGRA, w, h, hot_x, hot_y)`.
pub(crate) type CursorShapeEntry = (Vec<u8>, u32, u32, u32, u32);

/// What a present's commands read besides the framebuffer.
pub(crate) struct CpuFrame {
    /// Fragment uniforms (`SetFragmentBytes`): windowd-pushed shader parameters.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    fragment: [u8; 64],
    /// The cursor sprite `BlendCursor` composites (premultiplied BGRA, `cursor_w`×`cursor_h`);
    /// empty until windowd uploads one — the procedural arrow stands in.
    pub(crate) cursor: Vec<u8>,
    pub(crate) cursor_w: u32,
    pub(crate) cursor_h: u32,
    /// The hotspot of the active sprite (the GL and overlay cursors subtract it).
    pub(crate) cursor_hot: (u32, u32),
    /// Pre-uploaded shapes (`OP_UPLOAD_CURSOR_SHAPE`): a shape change is a 2-byte select, not a
    /// blocking re-upload. Slot = shape id.
    shapes: [Option<CursorShapeEntry>; nexus_display_proto::CURSOR_SHAPE_SLOTS],
}

/// The bytes a `w`×`h` BGRA sprite takes, if `bgra` holds at least that many.
fn sprite_len(bgra: &[u8], w: u32, h: u32) -> Result<usize, GfxError> {
    let needed = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    if needed == 0 || bgra.len() < needed {
        return Err(GfxError::InvalidArgument);
    }
    Ok(needed)
}

impl CpuFrame {
    pub(crate) fn new() -> Self {
        Self {
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            fragment: [0u8; 64],
            cursor: Vec::new(),
            cursor_w: 0,
            cursor_h: 0,
            cursor_hot: (0, 0),
            shapes: [const { None }; nexus_display_proto::CURSOR_SHAPE_SLOTS],
        }
    }

    /// Make `bgra` (`w`×`h`) the cursor sprite.
    pub(crate) fn store_cursor(&mut self, bgra: &[u8], w: u32, h: u32) -> Result<(), GfxError> {
        let needed = sprite_len(bgra, w, h)?;
        self.cursor.clear();
        self.cursor.extend_from_slice(&bgra[..needed]);
        (self.cursor_w, self.cursor_h) = (w, h);
        Ok(())
    }

    /// Fill shape slot `id`; arms nothing (`select_shape` activates a slot). A re-upload reuses
    /// the slot's allocation: the service's heap never frees.
    pub(crate) fn cache_shape(
        &mut self,
        id: u8,
        bgra: &[u8],
        w: u32,
        h: u32,
        hot: (u32, u32),
    ) -> Result<(), GfxError> {
        let slot = self.shapes.get_mut(id as usize).ok_or(GfxError::InvalidArgument)?;
        let needed = sprite_len(bgra, w, h)?;
        match slot {
            Some((buf, sw, sh, hx, hy)) => {
                buf.clear();
                buf.extend_from_slice(&bgra[..needed]);
                (*sw, *sh, *hx, *hy) = (w, h, hot.0, hot.1);
            }
            empty => {
                let mut buf = Vec::with_capacity(needed);
                buf.extend_from_slice(&bgra[..needed]);
                *empty = Some((buf, w, h, hot.0, hot.1));
            }
        }
        Ok(())
    }

    /// Make the cached shape `id` the cursor sprite (and its hotspot the active one). Alloc-free:
    /// the sprite's own allocation takes the copy.
    pub(crate) fn select_shape(&mut self, id: u8) -> Result<(), GfxError> {
        let Some(Some((buf, w, h, hot_x, hot_y))) = self.shapes.get(id as usize) else {
            return Err(GfxError::InvalidArgument);
        };
        let needed = sprite_len(buf, *w, *h)?;
        self.cursor.clear();
        self.cursor.extend_from_slice(&buf[..needed]);
        (self.cursor_w, self.cursor_h, self.cursor_hot) = (*w, *h, (*hot_x, *hot_y));
        Ok(())
    }
}

/// The framebuffer the commands land in: `len` bytes mapped at `fb`, rows of `stride_px` pixels,
/// the shared layout (`nexus_display_proto::layout`) — the display plane at its row.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) struct Target {
    fb: *mut u8,
    len: usize,
    stride_px: usize,
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
impl Target {
    /// # Safety
    /// `fb` points to `len` writable bytes this service maps for as long as the target is used,
    /// and nothing else holds a reference into them while a command executes.
    pub(crate) unsafe fn new(fb: *mut u8, len: usize, stride_px: usize) -> Self {
        Self { fb, len, stride_px }
    }

    /// The raw mapping, for the virgl path's GPU executors that read the framebuffer themselves.
    #[cfg(feature = "virgl")]
    pub(crate) fn raw(&self) -> (*mut u8, usize, usize) {
        (self.fb, self.len, self.stride_px)
    }
}

/// How a backdrop blur runs on the CPU: the box blur of the 2D path, or the separable gaussian
/// the virgl path falls back to when its GPU blur declines (the GPU kernel's shape).
#[cfg(all(feature = "os-lite", target_os = "none"))]
#[derive(Clone, Copy)]
pub(crate) enum Blur {
    Box,
    #[cfg(feature = "virgl")]
    Separable,
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
const DISPLAY_ROW: u32 = nexus_display_proto::layout::DISPLAY_ROW;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const DISPLAY_ROWS: u32 = nexus_display_proto::layout::PLANE_ROWS;

#[cfg(all(feature = "os-lite", target_os = "none"))]
impl CpuFrame {
    /// Execute one command into `t`. Screen-relative coordinates land in the display plane.
    pub(crate) fn execute(
        &mut self,
        t: &Target,
        cmd: &Command,
        blur: Blur,
    ) -> Result<(), GfxError> {
        let (fb, len, w) = (t.fb, t.len, t.stride_px);
        match cmd {
            Command::SetFragmentBytes { offset, data } => {
                let end = offset.saturating_add(data.len());
                if end > self.fragment.len() {
                    return Err(GfxError::CommandRejected);
                }
                self.fragment[*offset..end].copy_from_slice(data);
            }
            Command::DrawTiles { tiles, color } => {
                let c = color.as_array();
                for r in tiles {
                    let y = r.y.saturating_add(DISPLAY_ROW);
                    fill_rect_solid_vmo(fb, len, w, r.x, y, r.width, r.height, c);
                }
            }
            Command::FillSdfRoundedRect { rect, radius, color } => {
                let y = rect.y.saturating_add(DISPLAY_ROW);
                fill_sdf_rounded_vmo(
                    fb,
                    len,
                    w,
                    rect.x,
                    y,
                    rect.width,
                    rect.height,
                    *radius,
                    *color,
                );
            }
            Command::BlurBackdrop { rect, radius, saturation_percent } => {
                let y = rect.y.saturating_add(DISPLAY_ROW);
                let (rw, rh, sat) = (rect.width, rect.height, *saturation_percent);
                match blur {
                    Blur::Box => blur_backdrop_vmo(fb, len, w, rect.x, y, rw, rh, *radius, sat)?,
                    #[cfg(feature = "virgl")]
                    Blur::Separable => {
                        blur_backdrop_separable_vmo(fb, len, w, rect.x, y, rw, rh, *radius, sat)?
                    }
                }
            }
            // Retained-surface composite: `src_y` is an absolute row (the retained plane), `dst_y`
            // screen-relative.
            Command::BlitSurface { src_x, src_y, dst_x, dst_y, width, height } => {
                let dst_y = dst_y.saturating_add(DISPLAY_ROW);
                blit_vmo(fb, len, w, *src_x, *src_y, *dst_x, dst_y, *width, *height)?;
            }
            Command::BlendCursor { x, y, width, height } => {
                let y = y.saturating_add(DISPLAY_ROW);
                let (sprite, sw, sh) = (&self.cursor[..], self.cursor_w, self.cursor_h);
                blend_cursor_vmo(fb, len, w, *x, y, *width, *height, sprite, sw, sh)?;
            }
            // Raw blit: both rows absolute.
            Command::BlitAbsolute { src_x, src_y_abs, dst_x, dst_y_abs, width, height } => {
                blit_vmo(fb, len, w, *src_x, *src_y_abs, *dst_x, *dst_y_abs, *width, *height)?;
            }
            Command::FillSdfGradient { rect, radius, color_top, color_bottom } => {
                let y = rect.y.saturating_add(DISPLAY_ROW);
                let (rw, rh) = (rect.width, rect.height);
                crate::cpu_vector::fill_sdf_gradient_vmo(
                    fb,
                    len,
                    w,
                    rect.x,
                    y,
                    rw,
                    rh,
                    *radius,
                    *color_top,
                    *color_bottom,
                );
            }
            Command::DropShadow { rect, radius, blur, offset_x, offset_y, color } => {
                let y = rect.y.saturating_add(DISPLAY_ROW);
                crate::cpu_vector::drop_shadow_vmo(
                    fb,
                    len,
                    w,
                    rect.x,
                    y,
                    rect.width,
                    rect.height,
                    *radius,
                    *blur,
                    *offset_x,
                    *offset_y,
                    *color,
                    DISPLAY_ROW,
                    DISPLAY_ROWS,
                );
            }
            // A layer on the CPU: its shadow, its backdrop blurred in place when it is glass, then
            // its content alpha-blended over (opaque content blends to opaque). `opacity` and the
            // scroll/transform identities drive the GPU compositor only; the content's own alpha
            // carries translucency here.
            Command::CompositeLayer {
                src_row_abs,
                src_x,
                width,
                height,
                dst_x,
                dst_y,
                corner_radius,
                shadow_blur,
                shadow_offset_y,
                shadow_alpha,
                backdrop_blur,
                ..
            } => {
                let dst_y = dst_y.saturating_add(DISPLAY_ROW);
                if *shadow_blur > 0 {
                    crate::cpu_vector::drop_shadow_vmo(
                        fb,
                        len,
                        w,
                        *dst_x,
                        dst_y,
                        *width,
                        *height,
                        *corner_radius,
                        *shadow_blur,
                        0,
                        *shadow_offset_y,
                        RgbaColor::from_u32(((*shadow_alpha).min(255)) << 24),
                        DISPLAY_ROW,
                        DISPLAY_ROWS,
                    );
                }
                if *backdrop_blur > 0 {
                    let _ = blur_backdrop_vmo(
                        fb,
                        len,
                        w,
                        *dst_x,
                        dst_y,
                        *width,
                        *height,
                        *backdrop_blur,
                        0,
                    );
                }
                let _ = blit_blend_vmo(
                    fb,
                    len,
                    w,
                    *src_x,
                    *src_row_abs,
                    *dst_x,
                    dst_y,
                    *width,
                    *height,
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite(w: u32, h: u32, fill: u8) -> Vec<u8> {
        alloc::vec![fill; (w * h * 4) as usize]
    }

    #[test]
    fn a_selected_shape_becomes_the_sprite_and_its_hotspot() {
        let mut f = CpuFrame::new();
        f.cache_shape(3, &sprite(4, 2, 7), 4, 2, (1, 1)).unwrap();
        f.select_shape(3).unwrap();
        assert_eq!((f.cursor_w, f.cursor_h, f.cursor_hot), (4, 2, (1, 1)));
        assert_eq!(f.cursor, sprite(4, 2, 7));
        // The slot stays cached: a second select works without a re-upload.
        f.store_cursor(&sprite(1, 1, 9), 1, 1).unwrap();
        f.select_shape(3).unwrap();
        assert_eq!(f.cursor_w, 4);
    }

    /// The sizes come from windowd's frame — untrusted. A sprite shorter than its size, an empty
    /// size, a size whose byte count overflows: refused, the active sprite untouched.
    #[test]
    fn test_reject_a_sprite_shorter_than_its_size() {
        let mut f = CpuFrame::new();
        f.store_cursor(&sprite(2, 2, 1), 2, 2).unwrap();
        for (w, h) in [(3, 2), (0, 2), (2, 0), (u32::MAX, u32::MAX)] {
            assert!(f.store_cursor(&sprite(2, 2, 5), w, h).is_err(), "{w}x{h}");
            assert!(f.cache_shape(0, &sprite(2, 2, 5), w, h, (0, 0)).is_err(), "{w}x{h}");
        }
        assert_eq!((f.cursor_w, f.cursor_h), (2, 2));
        assert_eq!(f.cursor, sprite(2, 2, 1));
    }

    #[test]
    fn test_reject_a_shape_slot_outside_the_cache() {
        let mut f = CpuFrame::new();
        let past = nexus_display_proto::CURSOR_SHAPE_SLOTS as u8;
        assert!(f.cache_shape(past, &sprite(1, 1, 1), 1, 1, (0, 0)).is_err());
        assert!(f.select_shape(past).is_err());
        assert!(f.select_shape(0).is_err(), "an empty slot selects nothing");
    }
}
