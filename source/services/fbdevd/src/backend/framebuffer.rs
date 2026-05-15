// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Framebuffer VMO ownership and bounded row writes for `fbdevd`.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `fbdevd` host reject tests plus visible-bootstrap QEMU proofs.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use crate::error::{FbdevdError, Result};
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
use input_live_protocol::VisibleState;
use windowd::{
    validate_visible_bootstrap_capability, DisplayPresentHandoff, VisibleDisplayCapability,
};

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
use nexus_abi::{cap_query, vmo_create, vmo_write, CapQuery, Handle};

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
#[derive(Debug, Clone, Copy)]
pub struct FramebufferOwner {
    pub handle: Handle,
    pub base: u64,
    pub mode: windowd::VisibleBootstrapMode,
}

/// Blend the cursor bitmap into a framebuffer row.
///
/// The cursor is a BGRA8888 bitmap. For each pixel in the row that falls
/// within the cursor bounds, the cursor pixel (with alpha) is blended over
/// the framebuffer pixel. Pixels outside the cursor bounds are unchanged.
#[allow(dead_code)]
pub fn blend_cursor_row(
    row: &mut [u8],
    row_y: u32,
    cursor_bitmap: &[u8],
    cursor_width: u32,
    cursor_height: u32,
    cursor_x: i32,
    cursor_y: i32,
) {
    let cw = cursor_width as i32;
    let ch = cursor_height as i32;
    let row_y_i32 = row_y as i32;
    let cursor_row = row_y_i32 - cursor_y;
    if cursor_row < 0 || cursor_row >= ch {
        return;
    }

    let stride = row.len();
    for col in 0..(stride / 4) {
        let col_i32 = col as i32;
        let cursor_col = col_i32 - cursor_x;
        if cursor_col < 0 || cursor_col >= cw {
            continue;
        }
        let src_idx = ((cursor_row as u32 * cursor_width + cursor_col as u32) * 4) as usize;
        let dst_idx = col * 4;
        if src_idx + 4 > cursor_bitmap.len() || dst_idx + 4 > row.len() {
            continue;
        }
        let cb = cursor_bitmap[src_idx];
        let cg = cursor_bitmap[src_idx + 1];
        let cr = cursor_bitmap[src_idx + 2];
        let ca = cursor_bitmap[src_idx + 3];
        if ca == 0 {
            continue;
        }
        if ca == 255 {
            row[dst_idx] = cb;
            row[dst_idx + 1] = cg;
            row[dst_idx + 2] = cr;
            row[dst_idx + 3] = 255;
        } else {
            let alpha = ca as u32;
            let inv_alpha = 255 - alpha;
            let fb_b = row[dst_idx] as u32;
            let fb_g = row[dst_idx + 1] as u32;
            let fb_r = row[dst_idx + 2] as u32;
            row[dst_idx] = ((cb as u32 * alpha + fb_b * inv_alpha) / 255) as u8;
            row[dst_idx + 1] = ((cg as u32 * alpha + fb_g * inv_alpha) / 255) as u8;
            row[dst_idx + 2] = ((cr as u32 * alpha + fb_r * inv_alpha) / 255) as u8;
            row[dst_idx + 3] = 255;
        }
    }
}

pub fn validate_handoff(handoff: &DisplayPresentHandoff) -> Result<()> {
    let mode = handoff
        .mode
        .validate()
        .map_err(|_| FbdevdError::InvalidMode)?;
    if handoff
        .byte_len()
        .map_err(|_| FbdevdError::PresentWithoutFrame)?
        < mode.byte_len().map_err(|_| FbdevdError::InvalidMode)?
    {
        return Err(FbdevdError::PresentWithoutFrame);
    }
    Ok(())
}

pub fn validate_framebuffer_cap(
    mode: windowd::VisibleBootstrapMode,
    capability: VisibleDisplayCapability,
) -> Result<()> {
    let mode = mode.validate().map_err(|_| FbdevdError::InvalidMode)?;
    validate_visible_bootstrap_capability(mode, capability)
        .map_err(|_| FbdevdError::InvalidFramebufferCap)
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
impl FramebufferOwner {
    pub fn allocate(mode: windowd::VisibleBootstrapMode) -> Result<Self> {
        let mode = mode.validate().map_err(|_| FbdevdError::InvalidMode)?;
        let byte_len = mode.byte_len().map_err(|_| FbdevdError::InvalidMode)?;
        let handle = vmo_create(byte_len).map_err(|_| FbdevdError::FramebufferVmo)?;
        let mut query = CapQuery {
            kind_tag: 0,
            reserved: 0,
            base: 0,
            len: 0,
        };
        cap_query(handle, &mut query).map_err(|_| FbdevdError::InvalidFramebufferCap)?;
        validate_framebuffer_cap(
            mode,
            VisibleDisplayCapability {
                byte_len,
                mapped: true,
                writable: true,
            },
        )?;
        if query.kind_tag != 1 || query.len < byte_len as u64 {
            return Err(FbdevdError::InvalidFramebufferCap);
        }
        Ok(Self {
            handle,
            base: query.base,
            mode,
        })
    }

    pub fn write_handoff(&self, handoff: &DisplayPresentHandoff) -> Result<()> {
        validate_handoff(handoff)?;
        let row_len = self.mode.stride as usize;
        let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];
        for y in 0..self.mode.height {
            let offset = y as usize * row_len;
            handoff
                .copy_row(y, &mut row[..row_len])
                .map_err(|_| FbdevdError::FrameWrite)?;
            vmo_write(self.handle, offset, &row[..row_len]).map_err(|_| FbdevdError::FrameWrite)?;
        }
        Ok(())
    }

    pub fn write_live_visible_rows(
        &self,
        state: VisibleState,
        start_y: u32,
        end_y: u32,
        cursor_overlay: Option<(&[u8], u32, u32)>,
    ) -> Result<usize> {
        let row_len = self.mode.stride as usize;
        let start_y = start_y.min(self.mode.height);
        let end_y = end_y.min(self.mode.height);
        if start_y >= end_y {
            return Ok(0);
        }
        let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];
        for y in start_y..end_y {
            let offset = y as usize * row_len;
            windowd::copy_live_visible_row(state, self.mode, y, &mut row[..row_len])
                .map_err(|_| FbdevdError::FrameWrite)?;
            // Blend cursor overlay if available and row is within cursor vertical range.
            if let Some((bitmap, cw, ch)) = cursor_overlay {
                blend_cursor_row(
                    &mut row[..row_len],
                    y,
                    bitmap,
                    cw,
                    ch,
                    state.cursor_x,
                    state.cursor_y,
                );
            }
            vmo_write(self.handle, offset, &row[..row_len]).map_err(|_| FbdevdError::FrameWrite)?;
        }
        Ok((end_y - start_y) as usize * row_len)
    }
}
