// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SDF primitives for windowd compositor: circles, rounded rects, glass borders.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via compositor integration tests

use crate::error::WindowdError;
use crate::fixed_sdf;
use super::types::ProofBoxRect;
use super::{DARK_GLASS_RADIUS};

pub(crate) fn stroke_dark_glass_border_row(y: u32, row: &mut [u8], rect: ProofBoxRect, rc: super::types::RenderClip, stroke: u32, bgra: [u8; 4]) -> Result<(), WindowdError> {
    if stroke == 0 { return Ok(()); }
    let rp = (row.len() / 4) as u32;
    let st = rect.x.max(rc.start_x).min(rp);
    let en = rect.x.saturating_add(rect.width).min(rc.end_x).min(rp);
    if st >= en { return Ok(()); }
    let mnx = fixed_sdf::px_u32(rect.x); let mny = fixed_sdf::px_u32(rect.y);
    let mxx = fixed_sdf::px_u32(rect.x.saturating_add(rect.width));
    let mxy = fixed_sdf::px_u32(rect.y.saturating_add(rect.height));
    let rad = fixed_sdf::px_u32(DARK_GLASS_RADIUS);
    let py = fixed_sdf::pixel_center(y);
    for px in st..en {
        let sd = fixed_sdf::rounded_rect_sd(fixed_sdf::pixel_center(px), py, mnx, mny, mxx, mxy, rad);
            let a = fixed_sdf::border_alpha(sd, stroke);
        if a > 0 {
            let idx = px as usize * 4;
            let fa = (a as f32 * bgra[3] as f32 / 255.0).min(255.0) as u32;
            if fa == 0 { continue; }
            let inv = 255 - fa;
            row[idx] = ((bgra[0] as u32 * fa + row[idx] as u32 * inv) / 255) as u8;
            row[idx + 1] = ((bgra[1] as u32 * fa + row[idx + 1] as u32 * inv) / 255) as u8;
            row[idx + 2] = ((bgra[2] as u32 * fa + row[idx + 2] as u32 * inv) / 255) as u8;
        }
    }
    Ok(())
}

pub(crate) fn fill_sdf_circle_row(y: u32, row: &mut [u8], x: u32, ry: u32, w: u32, h: u32, bgra: [u8; 4]) -> Result<(), WindowdError> {
    let rp = (row.len() / 4) as u32;
    let cx = x as f32 + w as f32 * 0.5; let cy = ry as f32 + h as f32 * 0.5;
    let r = w.min(h) as f32 * 0.5;
    for px in x.max(0)..(x + w).min(rp) {
        let a = nexus_sdf::fill_alpha(nexus_sdf::sd_circle((px as f32 + 0.5, y as f32 + 0.5), (cx, cy), r), 1.0);
        if a > 0.0 { let idx = px as usize * 4; let fa = (a * bgra[3] as f32) as u32; if fa == 0 { continue; } let inv = 255 - fa; row[idx] = ((bgra[0] as u32 * fa + row[idx] as u32 * inv) / 255) as u8; row[idx + 1] = ((bgra[1] as u32 * fa + row[idx + 1] as u32 * inv) / 255) as u8; row[idx + 2] = ((bgra[2] as u32 * fa + row[idx + 2] as u32 * inv) / 255) as u8; }
    }
    Ok(())
}

pub(crate) fn stroke_sdf_circle_row(y: u32, row: &mut [u8], x: u32, ry: u32, w: u32, h: u32, stroke: u32, bgra: [u8; 4]) -> Result<(), WindowdError> {
    if stroke == 0 { return Ok(()); }
    let rp = (row.len() / 4) as u32;
    let cx = x as f32 + w as f32 * 0.5; let cy = ry as f32 + h as f32 * 0.5; let r = w.min(h) as f32 * 0.5;
    for px in x.max(0)..(x + w).min(rp) {
        let a = nexus_sdf::border_alpha(nexus_sdf::sd_circle((px as f32 + 0.5, y as f32 + 0.5), (cx, cy), r), stroke as f32, 1.0);
        if a > 0.0 { let idx = px as usize * 4; let fa = (a * bgra[3] as f32) as u32; if fa == 0 { continue; } let inv = 255 - fa; row[idx] = ((bgra[0] as u32 * fa + row[idx] as u32 * inv) / 255) as u8; row[idx + 1] = ((bgra[1] as u32 * fa + row[idx + 1] as u32 * inv) / 255) as u8; row[idx + 2] = ((bgra[2] as u32 * fa + row[idx + 2] as u32 * inv) / 255) as u8; }
    }
    Ok(())
}

pub(crate) fn fill_sdf_rounded_rect_row(y: u32, row: &mut [u8], rect: ProofBoxRect, cr: u32, bgra: [u8; 4]) -> Result<(), WindowdError> {
    let rp = (row.len() / 4) as u32;
    let crf = cr as f32;
    let min = (rect.x as f32, rect.y as f32); let max = ((rect.x + rect.width) as f32, (rect.y + rect.height) as f32);
    for px in rect.x.max(0)..(rect.x + rect.width).min(rp) {
        let a = nexus_sdf::fill_alpha(nexus_sdf::sd_rounded_rect((px as f32 + 0.5, y as f32 + 0.5), min, max, crf), 1.0);
        if a > 0.0 { let idx = px as usize * 4; let fa = (a * bgra[3] as f32) as u32; if fa == 0 { continue; } let inv = 255 - fa; row[idx] = ((bgra[0] as u32 * fa + row[idx] as u32 * inv) / 255) as u8; row[idx + 1] = ((bgra[1] as u32 * fa + row[idx + 1] as u32 * inv) / 255) as u8; row[idx + 2] = ((bgra[2] as u32 * fa + row[idx + 2] as u32 * inv) / 255) as u8; }
    }
    Ok(())
}

pub(crate) fn stroke_sdf_rounded_rect_row(y: u32, row: &mut [u8], rect: ProofBoxRect, cr: u32, stroke: u32, bgra: [u8; 4]) -> Result<(), WindowdError> {
    if stroke == 0 { return Ok(()); }
    let rp = (row.len() / 4) as u32;
    let crf = cr as f32;
    let min = (rect.x as f32, rect.y as f32); let max = ((rect.x + rect.width) as f32, (rect.y + rect.height) as f32);
    for px in rect.x.max(0)..(rect.x + rect.width).min(rp) {
        let a = nexus_sdf::border_alpha(nexus_sdf::sd_rounded_rect((px as f32 + 0.5, y as f32 + 0.5), min, max, crf), stroke as f32, 1.0);
        if a > 0.0 { let idx = px as usize * 4; let fa = (a * bgra[3] as f32) as u32; if fa == 0 { continue; } let inv = 255 - fa; row[idx] = ((bgra[0] as u32 * fa + row[idx] as u32 * inv) / 255) as u8; row[idx + 1] = ((bgra[1] as u32 * fa + row[idx + 1] as u32 * inv) / 255) as u8; row[idx + 2] = ((bgra[2] as u32 * fa + row[idx + 2] as u32 * inv) / 255) as u8; }
    }
    Ok(())
}
