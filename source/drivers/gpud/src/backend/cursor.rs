// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The cursor: the software sprite windowd's `BlendCursor` composites into the display plane
//! (the blend below; the sprite itself lives in `cpu_frame`, shared with the board's display
//! controller), the procedural arrow it falls back to before a sprite exists, and the virtio
//! GPU's hardware cursor overlay. The save-under cursor gpud once painted on its own (never
//! armed since windowd composites the cursor) is deleted.

#[cfg(all(feature = "os-lite", target_os = "none"))]
use super::raster::{blend_pixel_vmo, blend_premultiplied_vmo};
#[cfg(all(feature = "os-lite", target_os = "none"))]
use super::transport::ctrl_hdr;
use super::VirtioGpuBackend;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use crate::protocol;
use nexus_gfx::backend::error::GfxError;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use nexus_gfx::backend::traits::GfxBackend;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use nexus_gfx::backend::types::Rect;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use nexus_gfx::core::types::PixelFormat;

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
    pub fn arm_hw_cursor(
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
        self.cpu.cursor_hot = (hot_x, hot_y);
        Ok(())
    }

    /// Move the hardware cursor overlay. Requires a prior `arm_hw_cursor`.
    /// Host repositions the overlay — no scanout re-render, no guest composite.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn move_hw_cursor(&mut self, x: u32, y: u32) -> Result<(), GfxError> {
        let rid = self.cursor_resource_id.ok_or(GfxError::DeviceNotFound)?;
        let (hot_x, hot_y) = self.cpu.cursor_hot;
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
}
