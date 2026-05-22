// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Zero-copy shadow compositing pass for the windowd compositor.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test (shadow_cache_key)

use alloc::vec::Vec;
use crate::error::WindowdError;
use crate::fixed_sdf;
use crate::live_runtime::LayoutHotPathIndex;
use input_live_protocol::VisibleState;
use nexus_effects::ShadowArena;
use nexus_layout::LayoutResult;
use nexus_layout_types::Rgba8;
use super::cache::ShadowBoxCacheEntry;
use super::types::ProofBoxRect;
use super::{DARK_GLASS_RADIUS, SHADOW_BOX_CACHE_ENTRIES, SHADOW_CACHE_MAX_DOWNSCALE, SOFT_PANEL_SHADOW_ALPHA, SOFT_PANEL_SHADOW_BLUR_RADIUS, SOFT_PANEL_SHADOW_OFFSET_Y, WINDOWD_SHADOW_ARENA_SIZE};

pub(crate) fn shadow_cache_key(box_id_hash: u64, width: u32, height: u32, blur_radius: u32, spread: i32, color: Rgba8) -> u64 {
    let mut key = box_id_hash;
    key ^= (width as u64).rotate_left(7);
    key ^= (height as u64).rotate_left(17);
    key ^= (blur_radius as u64).rotate_left(29);
    key ^= (spread as u32 as u64).rotate_left(37);
    key ^= (color.r as u64).rotate_left(3);
    key ^= (color.g as u64).rotate_left(11);
    key ^= (color.b as u64).rotate_left(19);
    key ^ (color.a as u64).rotate_left(47)
}

pub(crate) fn shadow_cache_scale(width: u32, height: u32, budget_bytes: usize) -> Option<u8> {
    if width == 0 || height == 0 { return None; }
    for scale in 1..=SHADOW_CACHE_MAX_DOWNSCALE {
        let s = scale as u32;
        let cw = width.div_ceil(s).max(1);
        let ch = height.div_ceil(s).max(1);
        if cw as usize * ch as usize * 4 <= budget_bytes { return Some(scale); }
    }
    None
}

pub(crate) fn composite_shadow_layer_row(target: &mut [u8], layer_row: &[u8], cached_width: u32, logical_width: u32, scale: u8, shadow_x: i32, color: Rgba8) {
    let rp = target.len() / 4;
    let sc = scale.max(1) as usize;
    let ss = shadow_x.max(0).min(rp as i32) as usize;
    let se = shadow_x.saturating_add(logical_width as i32).max(0).min(rp as i32) as usize;
    let sr = color.r as u32; let sg = color.g as u32; let sb = color.b as u32; let sa = color.a as u32;
    for px in ss..se {
        let sp = (px as i32).saturating_sub(shadow_x) as usize / sc;
        if sp >= cached_width as usize { continue; }
        let ci = sp.saturating_mul(4);
        if ci + 4 > layer_row.len() { continue; }
        let la = layer_row[ci + 3] as u32;
        if la == 0 { continue; }
        let ta = (la * sa) / 255;
        if ta == 0 { continue; }
        let inv = 255 - ta;
        let idx = px * 4;
        target[idx] = ((sr * ta + target[idx] as u32 * inv) / 255) as u8;
        target[idx + 1] = ((sg * ta + target[idx + 1] as u32 * inv) / 255) as u8;
        target[idx + 2] = ((sb * ta + target[idx + 2] as u32 * inv) / 255) as u8;
    }
}

pub(crate) fn draw_soft_panel_shadow_row(y: u32, target: &mut [u8], rect: ProofBoxRect) {
    let rp = target.len() / 4;
    let blur = SOFT_PANEL_SHADOW_BLUR_RADIUS as i32;
    let sy = rect.y as i32 + SOFT_PANEL_SHADOW_OFFSET_Y;
    let sx = (rect.x as i32).saturating_sub(blur).max(0).min(rp as i32) as usize;
    let ex = (rect.x as i32).saturating_add(rect.width as i32).saturating_add(blur).max(0).min(rp as i32) as usize;
    if (y as i32) < sy.saturating_sub(blur) || (y as i32) > sy.saturating_add(rect.height as i32).saturating_add(blur) { return; }
    let mnx = fixed_sdf::px_u32(rect.x);
    let mny = fixed_sdf::px_i32(sy);
    let mxx = fixed_sdf::px_u32(rect.x.saturating_add(rect.width));
    let mxy = fixed_sdf::px_i32(sy.saturating_add(rect.height as i32));
    let rad = fixed_sdf::px_u32(DARK_GLASS_RADIUS);
    let py = fixed_sdf::pixel_center(y);
    for px in sx..ex {
        let sd = fixed_sdf::rounded_rect_sd(fixed_sdf::pixel_center(px as u32), py, mnx, mny, mxx, mxy, rad);
        let a = fixed_sdf::shadow_alpha_from_distance(sd.max(0), SOFT_PANEL_SHADOW_BLUR_RADIUS, SOFT_PANEL_SHADOW_ALPHA);
        if a == 0 { continue; }
        let inv = 255u32.saturating_sub(a);
        let idx = px * 4;
        target[idx] = (target[idx] as u32 * inv / 255) as u8;
        target[idx + 1] = (target[idx + 1] as u32 * inv / 255) as u8;
        target[idx + 2] = (target[idx + 2] as u32 * inv / 255) as u8;
    }
}

pub(crate) fn compute_shadow_row(_st: VisibleState, pl: Option<&LayoutResult>, pli: Option<&LayoutHotPathIndex>, y: u32, tgt: &mut [u8], ss: &mut [u8], bb: &mut [u8], arena: &mut ShadowArena<'_>, _cs: &mut [u8], sbc: &mut [ShadowBoxCacheEntry; SHADOW_BOX_CACHE_ENTRIES]) -> Result<(), WindowdError> {
    let Some(ly) = pl else { return Ok(()); };
    let rp = tgt.len() / 4;
    if ss.len() < tgt.len() || bb.len() < tgt.len() { return Err(WindowdError::BufferLengthMismatch); }
    let rm = pli.and_then(|i| if i.overflow_boxes() { None } else { let m = i.row_mask(y); (m != 0).then_some(m) });
    let mut ds = |lb: &nexus_layout::LayoutBox| {
        if lb.id == Some("combined_panels") { if let Some(r) = super::proof_box_rect(lb) { draw_soft_panel_shadow_row(y, tgt, r); } return; }
        let sh = match &lb.visual.shadow { Some(s) => s, None => return };
        let Some(re) = super::proof_box_rect(lb) else { return };
        let br = sh.blur_radius.0.max(0) as u32; let bi = br as i32;
        let sx = (re.x as i32).saturating_add(sh.offset_x.0).saturating_sub(sh.spread.0);
        let sy2 = (re.y as i32).saturating_add(sh.offset_y.0).saturating_sub(sh.spread.0);
        let sw = (re.width as i32).saturating_add(2 * sh.spread.0);
        let sh2 = (re.height as i32).saturating_add(2 * sh.spread.0);
        if sw <= 0 || sh2 <= 0 { return; }
        let cw = (sw + 2 * bi).max(0) as u32; let ch = (sh2 + 2 * bi).max(0) as u32;
        let bid = lb.id.map(|s| { let mut h: u64 = 0; for b in s.bytes() { h = h.wrapping_mul(31).wrapping_add(b as u64); } h }).unwrap_or(0);
        let key = shadow_cache_key(bid, cw, ch, br, 0, sh.color);
        let mut cs = None;
        for (i, e) in sbc.iter().enumerate() { if e.valid && e.key == key { cs = Some(i); break; } }
        if let Some(sl) = cs {
            let e = &sbc[sl];
            let layer_len = e.cache_height as usize * e.cache_width as usize * 4;
            let Some(lr) = arena.get(e.arena_offset, layer_len) else {
                return;
            };
            let ly2 = y.saturating_sub(re.y as u32); let cr2 = ly2 as usize / e.scale as usize;
            let rs = cr2 * e.cache_width as usize * 4; let re2 = rs + e.cache_width as usize * 4;
            if re2 <= lr.len() { composite_shadow_layer_row(tgt, &lr[rs..re2], e.cache_width, e.width, e.scale, sx, sh.color); }
            return;
        }
        let sc = shadow_cache_scale(cw, ch, WINDOWD_SHADOW_ARENA_SIZE).unwrap_or(SHADOW_CACHE_MAX_DOWNSCALE);
        let acw = cw.div_ceil(sc as u32).max(1); let ach = ch.div_ceil(sc as u32).max(1);
        let ab = acw as usize * ach as usize * 4;
        if let Some((off, buf)) = arena.alloc(ab) {
            for cy in 0..ach { draw_shadow_row_fallback((cy * sc as u32) as u32, &mut buf[cy as usize * acw as usize * 4..][..acw as usize * 4], ss, bb, (acw * 4 / 4) as usize, sx / sc as i32, sy2 / sc as i32, sw / sc as i32, sh2 / sc as i32, bi / sc as i32, br / sc as u32, sh.color); }
            super::blur_row_horizontal(buf, ab, br / sc as u32, bb);
            for e in sbc.iter_mut() { if !e.valid { e.key = key; e.arena_offset = off; e.width = cw; e.height = ch; e.cache_width = acw; e.cache_height = ach; e.scale = sc; e.valid = true; break; } }
            let ly2 = y.saturating_sub(re.y as u32); let cr2 = ly2 as usize / sc as usize;
            composite_shadow_layer_row(tgt, &buf[cr2 * acw as usize * 4..][..acw as usize * 4], acw, cw, sc, sx, sh.color);
        } else { draw_shadow_row_fallback(y, tgt, ss, bb, rp, sx, sy2, sw, sh2, bi, br, sh.color); }
    };
    match rm { Some(mut m) => while m != 0 { let bi = m.trailing_zeros() as usize; m &= m - 1; ds(&ly.boxes[bi]); }, None => for lb in &ly.boxes { ds(lb); } }
    Ok(())
}

pub(crate) fn draw_shadow_row_fallback(y: u32, tgt: &mut [u8], ss: &mut [u8], bb: &mut [u8], rp: usize, sx: i32, sy2: i32, sw: i32, sh2: i32, bi: i32, br: u32, col: Rgba8) {
    let yi = y as i32;
    let dy = if yi < sy2 { sy2.saturating_sub(yi) } else if yi >= sy2.saturating_add(sh2) { yi.saturating_sub(sy2.saturating_add(sh2).saturating_sub(1)) } else { 0 };
    if dy > bi { return; }
    let va = if br == 0 { 255 } else { let r = bi.saturating_add(1).saturating_sub(dy) as u32; (r * 255) / (br + 1) };
    if va == 0 { return; }
    let st = sx.saturating_sub(bi).max(0).min(rp as i32) as usize;
    let en = sx.saturating_add(sw).saturating_add(bi).max(0).min(rp as i32) as usize;
    if st >= en { return; }
    let sb = st * 4; let eb = en * 4; ss[sb..eb].fill(0);
    let cs = sx.max(0).min(rp as i32) as usize; let ce = sx.saturating_add(sw).max(0).min(rp as i32) as usize;
    for px in cs..ce { ss[px * 4 + 3] = va as u8; }
    let sl = eb.saturating_sub(sb);
    if br > 0 && sl != 0 { super::blur_row_horizontal(&mut ss[sb..eb], sl, br, bb); }
    let sr = col.r as u32; let sg = col.g as u32; let sb2 = col.b as u32; let sa = col.a as u32;
    for px in st..en {
        let idx = px * 4; let la = ss[idx + 3] as u32;
        if la == 0 { continue; }
        let ta = (la * sa) / 255; if ta == 0 { continue; }
        let inv = 255 - ta;
        tgt[idx] = ((sr * ta + tgt[idx] as u32 * inv) / 255) as u8;
        tgt[idx + 1] = ((sg * ta + tgt[idx + 1] as u32 * inv) / 255) as u8;
        tgt[idx + 2] = ((sb2 * ta + tgt[idx + 2] as u32 * inv) / 255) as u8;
    }
}
