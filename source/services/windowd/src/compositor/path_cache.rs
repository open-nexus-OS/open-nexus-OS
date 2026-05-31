// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-path-shape cache for windowd compositor: hash lookup + row blending.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test (path_cache_slot)

use super::cache::PathCacheEntry;
use super::types::ProofBoxRect;
use super::PATH_CACHE_MAX_SIDE;
use crate::error::WindowdError;

pub(crate) fn path_cache_slot(ih: u64, w: u32, h: u32, c: [u8; 4], cl: usize) -> usize {
    if cl == 0 {
        return 0;
    }
    (ih as usize)
        .wrapping_mul(131)
        .wrapping_add(w as usize * 17)
        .wrapping_add(h as usize * 7)
        .wrapping_add(u32::from_le_bytes(c) as usize)
        % cl
}

pub(crate) fn path_id_hash(id: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in id.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub(crate) fn blend_cached_path_row(
    y: u32,
    row: &mut [u8],
    id: Option<&str>,
    rect: ProofBoxRect,
    path: &nexus_layout_types::PathShape,
    color: [u8; 4],
    pc: &mut [PathCacheEntry],
) -> Result<bool, WindowdError> {
    let Some(id) = id else {
        return Ok(false);
    };
    if rect.width as usize > PATH_CACHE_MAX_SIDE || rect.height as usize > PATH_CACHE_MAX_SIDE {
        return Ok(false);
    }
    let ih = path_id_hash(id);
    let slot = path_cache_slot(ih, rect.width, rect.height, color, pc.len());
    let e = &mut pc[slot];
    let pl = rect.width as usize * rect.height as usize * 4;
    if !e.valid
        || e.id_hash != ih
        || e.width != rect.width
        || e.height != rect.height
        || e.color != color
    {
        e.pixels[..pl].fill(0);
        for cy in 0..rect.height {
            super::draw_path_row(
                cy,
                &mut e.pixels[cy as usize * rect.width as usize * 4..][..rect.width as usize * 4],
                0,
                0,
                rect.width,
                rect.height,
                path,
                color,
            )?;
        }
        e.id_hash = ih;
        e.width = rect.width;
        e.height = rect.height;
        e.color = color;
        e.valid = true;
    }
    super::blend_asset_row(y, row, rect.x, rect.y, rect.width, rect.height, &e.pixels[..pl])?;
    Ok(true)
}
