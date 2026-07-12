// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Software + hardware cursor: the save-under software cursor composited into
//! the scanout (visible on every display backend) and the virtio-gpu hardware
//! cursor overlay. Holds the cursor sprite store, the procedural arrow fallback
//! sprite, and the per-present paint/unpaint plumbing.

use super::VirtioGpuBackend;
use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::traits::GfxBackend;
use nexus_gfx::backend::types::Rect;
use nexus_gfx::core::types::PixelFormat;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use super::raster::{blend_pixel_vmo, blend_premultiplied_vmo};
#[cfg(all(feature = "os-lite", target_os = "none"))]
use super::transport::ctrl_hdr;
#[cfg(all(feature = "os-lite", target_os = "none"))]
#[allow(unused_imports)]
use crate::markers::GPUD_CURSOR_ON;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use crate::protocol;

#[cfg(all(feature = "os-lite", target_os = "none"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn blend_cursor_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    sprite: &[u8],
    sprite_w: u32,
    sprite_h: u32,
) -> Result<(), GfxError> {
    if w == 0 || h == 0 {
        return Ok(());
    }
    let fb_w_u = fb_w as u32;
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let copy_w = w.min(fb_w_u.saturating_sub(x));
    let copy_h = h.min(fb_h.saturating_sub(y));
    if copy_w == 0 || copy_h == 0 {
        return Ok(());
    }

    // Prefer the real uploaded cursor sprite (premultiplied BGRA from the Mocu
    // SVG). Fall back to the procedural arrow only until the sprite is uploaded.
    let use_sprite = !sprite.is_empty()
        && sprite_w > 0
        && sprite_h > 0
        && sprite.len() >= (sprite_w as usize * sprite_h as usize * 4);

    for py in 0..copy_h {
        for px in 0..copy_w {
            let idx = ((y as usize + py as usize) * fb_w + (x as usize + px as usize)) * 4;
            if idx + 4 > fb_len {
                continue;
            }
            if use_sprite {
                if px >= sprite_w || py >= sprite_h {
                    continue;
                }
                let s = (py as usize * sprite_w as usize + px as usize) * 4;
                let a = sprite[s + 3];
                if a == 0 {
                    continue;
                }
                // Source is premultiplied: out = src + dst*(255-a)/255.
                blend_premultiplied_vmo(fb, idx, &[sprite[s], sprite[s + 1], sprite[s + 2], a]);
            } else {
                let color = cursor_pixel_bgra(px, py, w, h);
                if color[3] == 0 {
                    continue;
                }
                blend_pixel_vmo(fb, idx, &color);
            }
        }
    }
    Ok(())
}

/// Footprint of the procedural [`CURSOR_ARROW`] fallback (drawn when no SVG cursor
/// sprite has been uploaded yet). Matches the arrow bitmap below.
pub(crate) const CURSOR_FALLBACK_W: u32 = 12;
pub(crate) const CURSOR_FALLBACK_H: u32 = 19;

/// Classic left-pointer arrow sprite, 12×19, tip at (0,0).
/// `B` = dark border, `W` = white fill, space = transparent. This is a fixed
/// crisp shape so the cursor reads as a normal pointer regardless of the 32×32
/// footprint windowd reserves — the opaque arrow occupies only the top-left.
#[cfg(all(feature = "os-lite", target_os = "none"))]
const CURSOR_ARROW: [&[u8; 12]; 19] = [
    b"B           ",
    b"BB          ",
    b"BWB         ",
    b"BWWB        ",
    b"BWWWB       ",
    b"BWWWWB      ",
    b"BWWWWWB     ",
    b"BWWWWWWB    ",
    b"BWWWWWWWB   ",
    b"BWWWWWWWWB  ",
    b"BWWWWWBBBBB ",
    b"BWWBWWB     ",
    b"BWB BWWB    ",
    b"BB  BWWB    ",
    b"B    BWWB   ",
    b"     BWWB   ",
    b"      BWWB  ",
    b"      BWWB  ",
    b"       BB   ",
];

/// Sample the arrow sprite at (px, py). Pixels outside the 12×19 shape (or in a
/// space cell) are fully transparent, so the cursor never fills its whole box.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) fn cursor_pixel_bgra(px: u32, py: u32, _w: u32, _h: u32) -> [u8; 4] {
    if py >= CURSOR_ARROW.len() as u32 || px >= 12 {
        return [0, 0, 0, 0];
    }
    match CURSOR_ARROW[py as usize][px as usize] {
        b'B' => [40, 40, 40, 255],    // soft dark border
        b'W' => [255, 255, 255, 255], // white fill
        _ => [0, 0, 0, 0],            // transparent
    }
}

impl VirtioGpuBackend {
    /// Record the pointer hotspot for the SW/GL cursor draw paths (the HW
    /// overlay path records it in `upload_cursor`). The GL scanout subtracts
    /// it when compositing the sprite, so centered-hotspot shapes (the
    /// resize pointers) sit exactly on the pointer position.
    pub fn set_cursor_hot(&mut self, hot_x: u32, hot_y: u32) {
        self.cursor_hot = (hot_x, hot_y);
    }

    /// Store the software cursor sprite (premultiplied BGRA) for BlendCursor.
    /// No hardware cursor resource, no UPDATE_CURSOR — avoids the QEMU virtio-gpu
    /// quirk. The sprite is composited into the display plane each frame.
    pub fn store_cursor_sprite(
        &mut self,
        bgra: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), GfxError> {
        let needed = (width as usize).saturating_mul(height as usize).saturating_mul(4);
        if needed == 0 || bgra.len() < needed {
            return Err(GfxError::InvalidArgument);
        }
        self.cursor_sprite.clear();
        self.cursor_sprite.extend_from_slice(&bgra[..needed]);
        self.cursor_sprite_w = width;
        self.cursor_sprite_h = height;
        Ok(())
    }

    /// Store a real icon sprite (premultiplied BGRA) plus its target position.
    /// Composited as a GPU sprite layer in the virgl buildup (`icon_tex_init` +
    /// `composite_icon_rt`), the same plumbing the cursor uses.
    #[allow(clippy::too_many_arguments)]
    pub fn store_icon_sprite(
        &mut self,
        bgra: &[u8],
        width: u32,
        height: u32,
        dst_x: u32,
        dst_y: u32,
        dst_w: u32,
        dst_h: u32,
    ) -> Result<(), GfxError> {
        let needed = (width as usize).saturating_mul(height as usize).saturating_mul(4);
        if needed == 0 || bgra.len() < needed {
            return Err(GfxError::InvalidArgument);
        }
        self.icon_sprite.clear();
        self.icon_sprite.extend_from_slice(&bgra[..needed]);
        self.icon_sprite_w = width;
        self.icon_sprite_h = height;
        self.icon_dst_x = dst_x;
        self.icon_dst_y = dst_y;
        // Fall back to the sprite's native size when no explicit dest size given.
        self.icon_dst_w = if dst_w == 0 { width } else { dst_w };
        self.icon_dst_h = if dst_h == 0 { height } else { dst_h };
        Ok(())
    }

    /// Fill a cursor shape-cache slot (OP_UPLOAD_CURSOR_SHAPE). Does NOT arm
    /// or switch anything — `select_cursor_shape` activates a cached slot.
    pub fn cache_cursor_shape(
        &mut self,
        shape_id: u8,
        bgra: &[u8],
        width: u32,
        height: u32,
        hot_x: u32,
        hot_y: u32,
    ) -> Result<(), GfxError> {
        let slot = shape_id as usize;
        if slot >= self.cursor_shape_cache.len() {
            return Err(GfxError::InvalidArgument);
        }
        let needed = (width as usize).saturating_mul(height as usize).saturating_mul(4);
        if needed == 0 || bgra.len() < needed {
            return Err(GfxError::InvalidArgument);
        }
        // Reuse the slot's existing allocation on re-upload (bump heap never
        // frees — clear+extend keeps the capacity instead of leaking a Vec).
        match &mut self.cursor_shape_cache[slot] {
            Some((buf, w, h, hx, hy)) => {
                buf.clear();
                buf.extend_from_slice(&bgra[..needed]);
                (*w, *h, *hx, *hy) = (width, height, hot_x, hot_y);
            }
            empty => {
                let mut buf = alloc::vec::Vec::with_capacity(needed);
                buf.extend_from_slice(&bgra[..needed]);
                *empty = Some((buf, width, height, hot_x, hot_y));
            }
        }
        Ok(())
    }

    /// Switch the active cursor sprite to a cached shape (OP_SELECT_CURSOR_SHAPE).
    /// GL/SW paths pick the new sprite up on their next present/paint; when the
    /// hardware overlay is armed the 64×64 cursor resource is refreshed too.
    /// Alloc-free on the hot path: the slot is taken and RETURNED (bump heap —
    /// a per-switch clone would leak 4KB per window-edge crossing).
    pub fn select_cursor_shape(&mut self, shape_id: u8) -> Result<(), GfxError> {
        let slot = shape_id as usize;
        if slot >= self.cursor_shape_cache.len() {
            return Err(GfxError::InvalidArgument);
        }
        let Some((buf, w, h, hot_x, hot_y)) = self.cursor_shape_cache[slot].take() else {
            return Err(GfxError::InvalidArgument);
        };
        let needed = (w as usize).saturating_mul(h as usize).saturating_mul(4);
        self.cursor_sprite.clear();
        self.cursor_sprite.extend_from_slice(&buf[..needed]);
        self.cursor_sprite_w = w;
        self.cursor_sprite_h = h;
        self.cursor_hot = (hot_x, hot_y);
        // Keep the save-under sized for the new sprite (SW paint path).
        let cap = needed.max(4);
        if self.cursor_saveunder.len() < cap {
            self.cursor_saveunder.resize(cap, 0);
        }
        // virgl GL scanout: the build-up samples the cursor from a GL TEXTURE
        // initialized from the sprite — refresh it or the shape switch stays
        // invisible (the sprite alone only feeds SW BlendCursor).
        #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
        let _ = self.cursor_tex_refresh();
        // HW overlay armed (mmio path): refresh the 64×64 cursor resource so
        // the host-composited pointer shows the new shape. Never reached on the
        // virgl GL scanout (upload_cursor is not armed there by design).
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        let hw_result = if self.hw_cursor_active() {
            self.upload_cursor(&buf, w, h, hot_x, hot_y)
        } else {
            Ok(())
        };
        #[cfg(not(all(feature = "os-lite", target_os = "none")))]
        let hw_result: Result<(), GfxError> = Ok(());
        // Return the taken slot (mem::take rule: every take needs its put-back).
        self.cursor_shape_cache[slot] = Some((buf, w, h, hot_x, hot_y));
        hw_result
    }

    /// Mark gpud as the cursor compositor and store the sprite/hotspot. The
    /// sprite stays the BlendCursor source; the first move paints it.
    pub fn cursor_take_ownership(&mut self, hot_x: u32, hot_y: u32) {
        self.cursor_owned = true;
        self.cursor_hot = (hot_x, hot_y);
        // Size the save-under for whichever sprite we'll paint: the uploaded SVG
        // sprite OR the procedural arrow fallback (so the fallback can erase its
        // own region on move without trailing).
        let sprite_px = self.cursor_sprite_w as usize * self.cursor_sprite_h as usize;
        let fallback_px = CURSOR_FALLBACK_W as usize * CURSOR_FALLBACK_H as usize;
        let cap = (sprite_px.max(fallback_px) * 4).max(4);
        if self.cursor_saveunder.len() < cap {
            self.cursor_saveunder.resize(cap, 0);
        }
    }

    /// Remove the cursor from the display plane (restore saved pixels). In-place,
    /// no flush — the caller flushes (or a present covers the region).
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) fn cursor_unpaint(&mut self) {
        if !self.cursor_drawn {
            return;
        }
        let (ox, oy, w, h) = (self.cursor_ox, self.cursor_oy, self.cursor_dw, self.cursor_dh);
        if let Some((fb, fb_len, fb_w, dyoff)) = self.scanout_fb() {
            for py in 0..h as usize {
                let sy = dyoff as usize + oy as usize + py;
                let dst = (sy * fb_w + ox as usize) * 4;
                let src = py * w as usize * 4;
                let n = w as usize * 4;
                if dst + n <= fb_len && src + n <= self.cursor_saveunder.len() {
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            self.cursor_saveunder.as_ptr().add(src),
                            fb.add(dst),
                            n,
                        );
                    }
                }
            }
        }
        self.cursor_drawn = false;
    }

    /// Save the scene pixels at (ox,oy) into the save-under buffer, then blend the
    /// sprite over them. In-place, no flush. Sets the drawn rect.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) fn cursor_paint(&mut self, ox: i32, oy: i32) {
        if !self.cursor_owned {
            return;
        }
        let Some((fb, fb_len, fb_w, dyoff)) = self.scanout_fb() else {
            return;
        };
        let fb_h = (fb_len / (fb_w * 4)) as i32 - dyoff as i32;
        let ox = ox.clamp(0, (fb_w as i32 - 1).max(0));
        let oy = oy.clamp(0, (fb_h - 1).max(0));
        // Use the uploaded SVG sprite if present; otherwise paint the procedural
        // arrow fallback (blend_cursor_vmo draws CURSOR_ARROW when the sprite is
        // empty). This keeps a visible pointer before/without the SVG cursor.
        let (sprite_w, sprite_h) = if self.cursor_sprite.is_empty() {
            (CURSOR_FALLBACK_W, CURSOR_FALLBACK_H)
        } else {
            (self.cursor_sprite_w, self.cursor_sprite_h)
        };
        let w = (sprite_w as i32).min(fb_w as i32 - ox).max(0) as u32;
        let h = (sprite_h as i32).min(fb_h - oy).max(0) as u32;
        if w == 0 || h == 0 {
            return;
        }
        // Save-under: copy current scene pixels into the buffer.
        for py in 0..h as usize {
            let sy = dyoff as usize + oy as usize + py;
            let src = (sy * fb_w + ox as usize) * 4;
            let dst = py * w as usize * 4;
            let n = w as usize * 4;
            if src + n <= fb_len && dst + n <= self.cursor_saveunder.len() {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        fb.add(src),
                        self.cursor_saveunder.as_mut_ptr().add(dst),
                        n,
                    );
                }
            }
        }
        // Blend the sprite over the display plane (premultiplied BGRA).
        let _ = blend_cursor_vmo(
            fb,
            fb_len,
            fb_w,
            ox as u32,
            dyoff + oy as u32,
            w,
            h,
            &self.cursor_sprite,
            self.cursor_sprite_w,
            self.cursor_sprite_h,
        );
        self.cursor_ox = ox;
        self.cursor_oy = oy;
        self.cursor_dw = w;
        self.cursor_dh = h;
        self.cursor_drawn = true;
    }

    /// Move the composited cursor to pointer position (px, py). Restores the old
    /// region, paints the new one, and flushes both to the display.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn cursor_move(&mut self, px: i32, py: i32) -> Result<(), GfxError> {
        if !self.cursor_owned {
            return Ok(());
        }
        let old = if self.cursor_drawn {
            Some((self.cursor_ox, self.cursor_oy, self.cursor_dw, self.cursor_dh))
        } else {
            None
        };
        self.cursor_unpaint();
        let ox = px - self.cursor_hot.0 as i32;
        let oy = py - self.cursor_hot.1 as i32;
        self.cursor_paint(ox, oy);
        // Flush the union of old and new cursor rects (screen-relative).
        let (nx, ny, nw, nh) = (self.cursor_ox, self.cursor_oy, self.cursor_dw, self.cursor_dh);
        match old {
            Some((oxo, oyo, owo, oho)) => {
                let x0 = oxo.min(nx).max(0);
                let y0 = oyo.min(ny).max(0);
                let x1 = (oxo + owo as i32).max(nx + nw as i32);
                let y1 = (oyo + oho as i32).max(ny + nh as i32);
                let _ = self.present_scanout_damage(Rect {
                    x: x0 as u32,
                    y: y0 as u32,
                    width: (x1 - x0).max(0) as u32,
                    height: (y1 - y0).max(0) as u32,
                });
            }
            None => {
                if nw > 0 && nh > 0 {
                    let _ = self.present_scanout_damage(Rect {
                        x: nx as u32,
                        y: ny as u32,
                        width: nw,
                        height: nh,
                    });
                }
            }
        }
        Ok(())
    }

    /// Before a windowd present: lift the cursor off the display so scene blits
    /// land on a cursor-free plane. Re-applied by `cursor_after_present`.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn cursor_before_present(&mut self) {
        if self.cursor_owned && self.cursor_drawn {
            self.cursor_unpaint();
            self.cursor_suspended = true;
        }
    }

    /// After a windowd present: re-save the (now current) scene under the cursor,
    /// blend the sprite back on top, and flush just the cursor rect.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn cursor_after_present(&mut self) {
        if !self.cursor_owned || !self.cursor_suspended {
            return;
        }
        self.cursor_suspended = false;
        self.cursor_paint(self.cursor_ox, self.cursor_oy);
        if self.cursor_dw > 0 && self.cursor_dh > 0 {
            let _ = self.present_scanout_damage(Rect {
                x: self.cursor_ox as u32,
                y: self.cursor_oy as u32,
                width: self.cursor_dw,
                height: self.cursor_dh,
            });
        }
    }

    /// Upload the cursor bitmap as a hardware cursor resource and arm the
    /// cursor-queue overlay (UPDATE_CURSOR).
    ///
    /// The virtio-gpu spec requires cursor resources to be exactly 64×64; QEMU
    /// silently ignores cursor data of any other size (the cursor shows as
    /// invisible — the historical "UPDATE_CURSOR quirk" was this, combined with
    /// transferring the resource BEFORE the bitmap was copied into its backing,
    /// so the host always sampled zeros). The sprite is copied into the top-left
    /// of a transparent 64×64 resource, transferred, and only then armed.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn upload_cursor(
        &mut self,
        bgra: &[u8],
        width: u32,
        height: u32,
        hot_x: u32,
        hot_y: u32,
    ) -> Result<(), GfxError> {
        const CURSOR_DIM: u32 = 64;
        if width == 0 || height == 0 || width > CURSOR_DIM || height > CURSOR_DIM {
            return Err(GfxError::InvalidArgument);
        }
        if bgra.len() < (width * height * 4) as usize {
            return Err(GfxError::InvalidArgument);
        }
        if self.cursorq.is_none() {
            return Err(GfxError::DeviceNotFound);
        }
        // Reuse the existing cursor resource on re-upload instead of leaking one.
        let rid = match self.cursor_resource_id {
            Some(rid) => rid,
            None => self.create_resource(CURSOR_DIM, CURSOR_DIM, PixelFormat::Bgra8888)?,
        };
        let record = self.find_resource(rid).ok_or(GfxError::InvalidArgument)?;
        // 1. Copy the sprite into the top-left of the 64×64 backing. The backing
        //    was zeroed at create, so the remainder stays fully transparent.
        let stride = (CURSOR_DIM * 4) as usize;
        let src_stride = (width * 4) as usize;
        unsafe {
            let dst = core::slice::from_raw_parts_mut(
                record.backing_va as *mut u8,
                stride * CURSOR_DIM as usize,
            );
            for row in 0..height as usize {
                let s = row * src_stride;
                let d = row * stride;
                dst[d..d + src_stride].copy_from_slice(&bgra[s..s + src_stride]);
            }
        }
        // 2. Transfer guest backing → host resource (must follow the copy).
        let full = Rect { x: 0, y: 0, width: CURSOR_DIM, height: CURSOR_DIM };
        self.transfer_to_host_os(record, full)?;
        // 3. Arm the hardware cursor overlay on the cursor queue.
        let cmd = protocol::VirtioGpuUpdateCursor {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_UPDATE_CURSOR),
            pos: protocol::VirtioGpuCursorPosData { scanout_id: 0, x: 0, y: 0, _padding: 0 },
            resource_id: rid.0,
            hot_x,
            hot_y,
            _padding: 0,
        };
        self.cursor_submit_struct(&cmd)?;
        self.cursor_resource_id = Some(rid);
        self.cursor_hot = (hot_x, hot_y);
        Ok(())
    }

    /// Move the hardware cursor overlay. Requires a prior `upload_cursor`.
    /// Host repositions the overlay — no scanout re-render, no guest composite.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn move_hw_cursor(&mut self, x: u32, y: u32) -> Result<(), GfxError> {
        let rid = self.cursor_resource_id.ok_or(GfxError::DeviceNotFound)?;
        let (hot_x, hot_y) = self.cursor_hot;
        let cmd = protocol::VirtioGpuCursorPos {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_MOVE_CURSOR),
            pos: protocol::VirtioGpuCursorPosData { scanout_id: 0, x, y, _padding: 0 },
            resource_id: rid.0,
            hot_x,
            hot_y,
            _padding: 0,
        };
        self.cursor_submit_struct(&cmd)
    }

    /// True once the hardware cursor overlay is armed.
    pub fn hw_cursor_active(&self) -> bool {
        self.cursor_resource_id.is_some()
    }

    /// Records the current pointer position for the GL-scanout fallback cursor
    /// (the Stage-4 build-up draws the procedural arrow at `cursor_ox/oy` each
    /// present). Transfer-free, so it is safe on the virgl GL scanout.
    pub fn set_pointer_pos(&mut self, x: i32, y: i32) {
        self.cursor_ox = x;
        self.cursor_oy = y;
    }

    /// Arms the hardware-cursor overlay with the procedural [`CURSOR_ARROW`] so a
    /// pointer is visible WITHOUT an uploaded SVG sprite — a testing fallback that
    /// is independent of windowd's BlendCursor, the scanout, and the build-up
    /// (the overlay is a QEMU-composited plane). Tip at (0,0) = hot spot.
    ///
    /// NOTE: `upload_cursor` issues a `transfer_to_host` for the cursor resource,
    /// which blanks the virgl GL-scanout present — DO NOT call this on the virgl
    /// path; it is kept for the CPU/mmio scanout where the transfer is harmless.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    #[allow(dead_code)]
    pub fn install_fallback_hw_cursor(&mut self) -> Result<(), GfxError> {
        let w = CURSOR_FALLBACK_W;
        let h = CURSOR_FALLBACK_H;
        let mut sprite = alloc::vec::Vec::new();
        sprite.resize((w * h * 4) as usize, 0u8);
        for py in 0..h {
            for px in 0..w {
                let c = cursor_pixel_bgra(px, py, w, h);
                let i = ((py * w + px) * 4) as usize;
                sprite[i..i + 4].copy_from_slice(&c);
            }
        }
        self.upload_cursor(&sprite, w, h, 0, 0)?;
        let _ = nexus_abi::debug_println(crate::markers::GPUD_CURSOR_ON);
        Ok(())
    }
}
