// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Low-level rendering primitives. RISC-V optimized: div255 via multiply+shift.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable

use crate::error::WindowdError;

#[inline]
fn div255(x: u32) -> u8 {
    ((x * 257 + 32768) >> 16) as u8
}

pub(crate) fn blend_asset_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    top: u32,
    w: u32,
    h: u32,
    src: &[u8],
) -> Result<(), WindowdError> {
    if y < top || y >= top.saturating_add(h) {
        return Ok(());
    }
    let sy = y - top;
    let sr = sy as usize * w as usize * 4;
    blend_overlay_row(
        row,
        x as usize,
        src.get(sr..sr + w as usize * 4).ok_or(WindowdError::BufferLengthMismatch)?,
    )
}

pub(crate) fn blend_asset_row_clipped(
    y: u32,
    row: &mut [u8],
    x: u32,
    top: u32,
    w: u32,
    h: u32,
    src: &[u8],
    cx: u32,
    cw: u32,
) -> Result<(), WindowdError> {
    if y < top || y >= top.saturating_add(h) || cw == 0 {
        return Ok(());
    }
    let vx = x.max(cx);
    let ve = x.saturating_add(w).min(cx.saturating_add(cw)).min((row.len() / 4) as u32);
    if ve <= vx {
        return Ok(());
    }
    let sy = y - top;
    let sr = sy as usize * w as usize * 4;
    let so = vx.saturating_sub(x) as usize * 4;
    let sl = ve.saturating_sub(vx) as usize * 4;
    blend_overlay_row(
        row,
        vx as usize,
        src.get(sr + so..sr + so + sl).ok_or(WindowdError::BufferLengthMismatch)?,
    )
}

pub(crate) fn fill_row_rect(
    y: u32,
    row: &mut [u8],
    x: u32,
    ry: u32,
    w: u32,
    h: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if y < ry || y >= ry.saturating_add(h) {
        return Ok(());
    }
    let rp = row.len() / 4;
    let st = x.min(rp as u32) as usize;
    let en = x.saturating_add(w).min(rp as u32) as usize;
    for px in st..en {
        let idx = px.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
        let a = u32::from(bgra[3]);
        if a == 255 {
            row[idx..idx + 4].copy_from_slice(&[bgra[0], bgra[1], bgra[2], 0xff]);
            continue;
        }
        if a == 0 {
            continue;
        }
        let inv = 255u32.saturating_sub(a);
        row[idx] = div255(u32::from(bgra[0]) * a + u32::from(row[idx]) * inv);
        row[idx + 1] = div255(u32::from(bgra[1]) * a + u32::from(row[idx + 1]) * inv);
        row[idx + 2] = div255(u32::from(bgra[2]) * a + u32::from(row[idx + 2]) * inv);
        row[idx + 3] = 0xff;
    }
    Ok(())
}

pub(crate) fn fill_triangle_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    ry: u32,
    w: u32,
    h: u32,
    up: bool,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if w == 0 || h == 0 || y < ry || y >= ry.saturating_add(h) {
        return Ok(());
    }
    let ly = y - ry;
    let p = if up { h.saturating_sub(ly + 1) } else { ly };
    let sp = ((p + 1) * w).max(h) / h.max(1);
    let sp = sp.max(1).min(w);
    fill_row_rect(y, row, x + (w.saturating_sub(sp)) / 2, y, sp, 1, bgra)
}

pub(crate) fn draw_path_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    ry: u32,
    w: u32,
    h: u32,
    path: &nexus_layout_types::PathShape,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if w == 0 || h == 0 || path.points.len() < 2 || y < ry || y >= ry.saturating_add(h) {
        return Ok(());
    }
    for seg in path.points.windows(2) {
        draw_line_segment_row(y, row, x, ry, w, h, seg[0], seg[1], bgra)?;
    }
    if path.closed {
        draw_line_segment_row(
            y,
            row,
            x,
            ry,
            w,
            h,
            *path.points.last().unwrap_or(&nexus_layout_types::PathPoint::new(0, 0)),
            path.points[0],
            bgra,
        )?;
    }
    Ok(())
}

pub(crate) fn draw_line_segment_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    ry: u32,
    w: u32,
    h: u32,
    s: nexus_layout_types::PathPoint,
    e: nexus_layout_types::PathPoint,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    let x0 = x + (u32::from(s.x_milli) * w) / 1000;
    let y0 = ry + (u32::from(s.y_milli) * h) / 1000;
    let x1 = x + (u32::from(e.x_milli) * w) / 1000;
    let y1 = ry + (u32::from(e.y_milli) * h) / 1000;
    if y < y0.min(y1) || y > y0.max(y1) {
        return Ok(());
    }
    if y0 == y1 {
        let sx2 = x0.min(x1);
        return fill_row_rect(
            y,
            row,
            sx2,
            y,
            x0.max(x1).saturating_sub(sx2).saturating_add(1),
            1,
            bgra,
        );
    }
    let dy = y1 as i64 - y0 as i64;
    let dx = x1 as i64 - x0 as i64;
    let px = x0 as i64 + dx * (y as i64 - y0 as i64) / dy;
    fill_row_rect(y, row, (px.max(0) as u32).saturating_sub(1), y, 3, 1, bgra)
}

pub(crate) fn stroke_row_rect_width(
    y: u32,
    row: &mut [u8],
    x: u32,
    ry: u32,
    w: u32,
    h: u32,
    stroke: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if w == 0 || h == 0 || stroke == 0 {
        return Ok(());
    }
    let st = stroke.min(w).min(h);
    fill_row_rect(y, row, x, ry, w, st, bgra)?;
    fill_row_rect(y, row, x, ry + h.saturating_sub(st), w, st, bgra)?;
    fill_row_rect(y, row, x, ry, st, h, bgra)?;
    fill_row_rect(y, row, x + w.saturating_sub(st), ry, st, h, bgra)
}

pub(crate) fn rgba_to_bgra(c: nexus_layout_types::Rgba8) -> [u8; 4] {
    [c.b, c.g, c.r, c.a]
}

pub(crate) fn blend_overlay_row(row: &mut [u8], x: usize, src: &[u8]) -> Result<(), WindowdError> {
    let rp = row.len() / 4;
    for (col, px) in src.chunks_exact(4).enumerate() {
        let dc = x.saturating_add(col);
        if dc >= rp {
            break;
        }
        let a = px[3];
        if a == 0 {
            continue;
        }
        let dst = dc.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
        if a == 255 {
            row[dst..dst + 4].copy_from_slice(px);
            continue;
        }
        let a = u32::from(a);
        let inv = 255u32.saturating_sub(a);
        for ch in 0..3 {
            row[dst + ch] = div255(u32::from(px[ch]) * a + u32::from(row[dst + ch]) * inv);
        }
        row[dst + 3] = 255;
    }
    Ok(())
}
