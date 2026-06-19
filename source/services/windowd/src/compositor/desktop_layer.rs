// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shell-P2b/P3 — render the desktop-shell scene into an off-screen
//! atlas surface as ONE opaque layer, composited onto the scanout via the GPU
//! layer path (`try_composite_layer`). This is the path that actually reaches
//! the virgl scanout: content written to the retained Plane 1 is NOT presented
//! on virgl (only explicitly-composited RT layers are — see
//! [[black-screen-is-2d-3d-dual-not-host]]), so the chat window and now the
//! shell are composited as layers, exactly like the chat atlas.
//! OWNERS: @ui
//! STATUS: In progress (P2b)
//!
//! The whole `desktop_root` panel is opaque (themed Background fill), so the
//! layer is opaque and the wallpaper shows only OUTSIDE the root rect — no
//! transparent-gap problem (the layer composite samples the atlas as opaque
//! RGB + its own rounded mask). Boxes render at **layout-local** coordinates
//! (origin 0,0); the composite places the whole atlas at the scene origin.

use super::primitives::{fill_row_rect, rgba_to_bgra, stroke_row_rect_width};
use super::sdf::{fill_sdf_rounded_rect_row, stroke_sdf_rounded_rect_row};
use super::types::ProofBoxRect;
use crate::error::WindowdError;
use nexus_layout::LayoutResult;

/// Draw one atlas row (`local_y`, layout-local) of the desktop shell scene into
/// `row`. Boxes are painted parent-before-child (LayoutResult order) so the
/// opaque root fills the band first and children layer on top. Backgrounds and
/// borders only — text/icons land in later phases.
pub(crate) fn draw_desktop_shell_row(
    local_y: u32,
    row: &mut [u8],
    layout: &LayoutResult,
) -> Result<(), WindowdError> {
    for layout_box in &layout.boxes {
        let width = layout_box.rect.width.as_u32().unwrap_or(0);
        let height = layout_box.rect.height.as_u32().unwrap_or(0);
        if width == 0 || height == 0 {
            continue;
        }
        let x = layout_box.rect.x.as_u32().unwrap_or(0);
        let y = layout_box.rect.y.as_u32().unwrap_or(0);
        let rect = ProofBoxRect { x, y, width, height };
        if !rect.contains_y(local_y) {
            continue;
        }
        let cr = layout_box.visual.corner_radius.top_left.as_u32().unwrap_or(0);
        if let Some(bg) = layout_box.visual.background {
            let bgra = rgba_to_bgra(bg);
            if cr > 0 {
                fill_sdf_rounded_rect_row(local_y, row, rect, cr, bgra)?;
            } else {
                fill_row_rect(local_y, row, x, y, width, height, bgra)?;
            }
        }
        if let Some(border) = layout_box.visual.border.top {
            let bw = border.width.as_u32().unwrap_or(1);
            let bc = rgba_to_bgra(border.color);
            if cr > 0 {
                stroke_sdf_rounded_rect_row(local_y, row, rect, cr, bw, bc)?;
            } else {
                stroke_row_rect_width(local_y, row, x, y, width, height, bw, bc)?;
            }
        }
    }
    Ok(())
}

/// Root panel dimensions (the `desktop_root` box) for sizing the atlas surface
/// and the composite. Falls back to the whole layout bounds if the root id is
/// absent.
pub(crate) fn shell_root_dims(layout: &LayoutResult) -> (u32, u32) {
    if let Some(root) = layout.boxes.iter().find(|b| b.id == Some("desktop_root")) {
        let w = root.rect.width.as_u32().unwrap_or(0);
        let h = root.rect.height.as_u32().unwrap_or(0);
        if w > 0 && h > 0 {
            return (w, h);
        }
    }
    let w = layout
        .boxes
        .iter()
        .map(|b| b.rect.x.as_u32().unwrap_or(0) + b.rect.width.as_u32().unwrap_or(0))
        .max()
        .unwrap_or(0);
    (w, layout.content_height.as_u32().unwrap_or(0))
}
