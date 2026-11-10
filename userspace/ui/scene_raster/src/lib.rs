// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `nexus-scene-raster` — the ONE CPU scene painter for laid-out design-system
//! scenes: `LayoutBox` lists → BGRA pixels, row by row.
//!
//! PROMOTED from the goldens-harness painter (`tests/ui_v10_goldens`) per the
//! promote-best rule: the harness proved this exact fill model against the
//! committed goldens (rounded-rect corner test, even-odd polygon fill for
//! `ShapeKind::{Circle,Triangle*,Path,Vector}`, per-edge borders, src-over
//! blending so translucent glass reads over the base). The harness now calls
//! THIS crate, and the `app-host` DSL renderer streams its surface rows
//! through it — on-device pixels match the goldens by construction.
//!
//! ROW MODEL: callers stream one scanline at a time (`paint_row`) — the
//! app-host renders banded into its surface VMO without a full-frame buffer.
//! Backdrop blur and drop shadows are NOT this painter's job (compositor/GPU
//! features — `nexus-gfx` `LayerBackdrop`/shadow on the live path); text
//! glyphs blend in the separate baked-text pass.

mod borders;
use borders::{paint_borders_row, paint_inset_highlight_row};

pub mod anim;
pub use anim::NodeAnim;

mod scrolled;
pub use scrolled::{
    paint_row, paint_row_picked, paint_row_picked_animated, paint_row_picked_indexed, ScrollView,
};

mod shapes;
use shapes::sqrt_f32;
#[cfg(test)]
mod span_identity;

use nexus_layout::LayoutBox;
use nexus_layout_types::{Material, Rgba8, ShapeKind};

/// One BGRA scanline of the target surface.
pub struct RowCanvas<'a> {
    /// The row's pixel bytes (BGRA, tight — `width * 4`).
    pub buf: &'a mut [u8],
    /// The row's y in surface coordinates.
    pub y: i32,
    /// Surface width in px (clip bound).
    pub width: i32,
    /// Horizontal paint shift (scroll): a pixel painted at model-x lands at
    /// `x - shift_x` on the surface. 0 = identity (the common case).
    pub shift_x: i32,
    /// Horizontal scissor on the SURFACE (x0, x1 exclusive) — the scroll
    /// viewport's columns. `None` = full row.
    pub clip_x: Option<(i32, i32)>,
}

impl RowCanvas<'_> {
    /// A plain unscrolled row canvas.
    #[must_use]
    pub fn new(buf: &mut [u8], y: i32, width: i32) -> RowCanvas<'_> {
        RowCanvas { buf, y, width, shift_x: 0, clip_x: None }
    }

    /// Src-over blend one pixel of this row (model-x; scroll shift + viewport
    /// scissor applied here so every shape/text painter inherits them).
    #[inline]
    pub fn blend(&mut self, x: i32, c: Rgba8) {
        let x = x - self.shift_x;
        if let Some((x0, x1)) = self.clip_x {
            if x < x0 || x >= x1 {
                return;
            }
        }
        if x < 0 || x >= self.width || c.a == 0 {
            return;
        }
        let i = (x * 4) as usize;
        if i + 4 > self.buf.len() {
            return;
        }
        let (a, inv) = (c.a as u32, 255 - c.a as u32);
        let mix = |dst: u8, src: u8| ((dst as u32 * inv + src as u32 * a) / 255) as u8;
        self.buf[i] = mix(self.buf[i], c.b);
        self.buf[i + 1] = mix(self.buf[i + 1], c.g);
        self.buf[i + 2] = mix(self.buf[i + 2], c.r);
        self.buf[i + 3] = (a + self.buf[i + 3] as u32 * inv / 255) as u8;
    }

    /// REPLACE one pixel of this row (alpha included; same shift/scissor
    /// discipline as [`blend`](Self::blend)) — the glass-region reset write.
    #[inline]
    pub fn set(&mut self, x: i32, c: Rgba8) {
        let x = x - self.shift_x;
        if let Some((x0, x1)) = self.clip_x {
            if x < x0 || x >= x1 {
                return;
            }
        }
        if x < 0 || x >= self.width {
            return;
        }
        let i = (x * 4) as usize;
        if i + 4 > self.buf.len() {
            return;
        }
        // Premultiplied write — the exact pixel `blend` produces onto an
        // empty destination, so a glass reset is indistinguishable from
        // painting the tint on a fresh surface.
        let a = c.a as u32;
        self.buf[i] = (c.b as u32 * a / 255) as u8;
        self.buf[i + 1] = (c.g as u32 * a / 255) as u8;
        self.buf[i + 2] = (c.r as u32 * a / 255) as u8;
        self.buf[i + 3] = c.a;
    }
}

/// Paint one box's contribution to this row (fill + borders).
pub fn paint_box_row(canvas: &mut RowCanvas<'_>, b: &LayoutBox) {
    let (x, y, w, h) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
    paint_box_row_at(canvas, b, x, y, w, h, b.visual.background, 100);
}

/// The row's flat color of a vertical linear gradient: a row-based painter
/// renders `linear-gradient(to bottom, top, bottom)` EXACTLY as one lerped
/// color per row — no banding beyond 8-bit quantization, zero extra passes.
#[inline]
fn gradient_row_color(
    top: nexus_layout_types::Rgba8,
    bottom: nexus_layout_types::Rgba8,
    row: i32,
    y: i32,
    h: i32,
) -> nexus_layout_types::Rgba8 {
    let t_num = (row - y).clamp(0, h.max(1) - 1);
    let t_den = (h.max(1) - 1).max(1);
    // Signed-safe integer lerp per channel.
    let ch = |a: u8, b: u8| -> u8 {
        let ai = a as i32;
        let bi = b as i32;
        (ai + (bi - ai) * t_num / t_den) as u8
    };
    nexus_layout_types::Rgba8 {
        r: ch(top.r, bottom.r),
        g: ch(top.g, bottom.g),
        b: ch(top.b, bottom.b),
        a: ch(top.a, bottom.a),
    }
}

/// [`paint_box_row`] at an EXPLICIT geometry + background: the shared shape
/// dispatch used by the un-animated paint (the box's own rect) and the per-node
/// ANIMATION path (the transformed rect + faded fill) — every `ShapeKind`
/// (rect/triangles/circle/path/vector) scales and translates, so icons and
/// round buttons animate as whole shapes, not just their bounding fill.
/// `radius_pct` scales the corner radius (100 = as authored).
// reason: shared shape-dispatch entry — args are the explicit geometry, fill
// and radius scale; a struct wrapper would not improve this hot path.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_box_row_at(
    canvas: &mut RowCanvas<'_>,
    b: &LayoutBox,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    background: Option<Rgba8>,
    radius_pct: u16,
) {
    if w <= 0 || h <= 0 {
        return;
    }
    // Soft drop shadow BEHIND the box (design elevation, `.shadow(t)`):
    // painted first so the box fill covers its own footprint. Analytic
    // rounded-rect SDF with a linear falloff over the blur radius — a
    // one-shot cost at (re)render time, never per frame.
    //
    // A compositor region's shadow is the compositor's: app-host declares a blur root (and a
    // fully opaque pane/overlay root) as a region with its shadow, and windowd draws that shadow
    // under the region, blurred on the GPU; painting it here as well doubled it.
    // A box's own shadow reaches the rows above and below it (`shadow_reach`): such a row gets
    // the shadow and nothing else — leaving at the box's edge, the shadow ended flat there.
    // A faded box paints every colour at its effective opacity (`LayoutBox::opacity`, its own
    // times its ancestors'), so a disabled control dims as one.
    let op = u32::from(b.opacity);
    let fade =
        |c: Rgba8| if op == 255 { c } else { Rgba8 { a: (u32::from(c.a) * op / 255) as u8, ..c } };
    let (_, above, _, below) = shadow_reach(b);
    if canvas.y < y - above || canvas.y >= y + h + below {
        return;
    }
    if let Some(shadow) = b.visual.shadow.filter(|_| !is_region(b)) {
        let shadow = nexus_layout_types::BoxShadow { color: fade(shadow.color), ..shadow };
        paint_shadow_row(canvas, &shadow, b, x, y, w, h);
    }
    if canvas.y < y || canvas.y >= y + h {
        return;
    }
    // A vertical gradient wins over the flat fill: substitute this row's
    // lerped color and reuse every shape path unchanged. An overlay nested in another surface
    // cannot blur (one blur per stack): its glass is drawn at full strength, opaque.
    let nested_overlay = b.glass_nested && b.visual.material == Some(Material::Overlay);
    let background = match b.visual.background_gradient {
        Some((top, bottom)) => Some(gradient_row_color(top, bottom, canvas.y, y, h)),
        None => background,
    }
    .map(|c| if nested_overlay { Rgba8 { a: 255, ..c } } else { c })
    .map(fade);
    if let Some(bg) = background {
        let (xf, yf, wf, hf) = (x as f32, y as f32, w as f32, h as f32);
        match &b.visual.shape {
            ShapeKind::Rect => {
                let radius = (b.visual.corner_radius.top_left.0.max(0) as i64
                    * radius_pct.max(1) as i64
                    / 100) as i32;
                // A glass ROOT resets its rect to the pure tint (alpha
                // included) instead of src-over: content the surface painted
                // beneath the region must not bake through — the COMPOSITOR
                // supplies the backdrop (destination-so-far blur of
                // everything already composited under the region). NESTED
                // glass (a card row on a blur surface) blends
                // src-over instead: it sits on its parent's tint, and only
                // the root's single region asks the compositor for blur.
                // Replacing unconditionally is how every glass stack showed
                // nothing but the wallpaper — each inner rect erased its
                // parent's pixels and the only backdrop left was the
                // window's.
                // A card is a translucent layer and always blends; a blur root takes the
                // compositor's backdrop and an opaque surface covers what is under it.
                if b.visual.material.is_some_and(|m| m.blurs() || m.opaque()) && !b.glass_nested {
                    let r = radius.max(0).min(w / 2).min(h / 2);
                    canvas.fill_round_rect_row_replace(x, y, w, h, r, bg);
                } else {
                    canvas.fill_round_rect_row(x, y, w, h, radius, bg);
                }
            }
            ShapeKind::TriangleUp => {
                let pts = [(xf + wf / 2.0, yf), (xf + wf, yf + hf), (xf, yf + hf)];
                canvas.fill_polygon_row(3, |i| pts[i], bg);
            }
            ShapeKind::TriangleDown => {
                let pts = [(xf, yf), (xf + wf, yf), (xf + wf / 2.0, yf + hf)];
                canvas.fill_polygon_row(3, |i| pts[i], bg);
            }
            ShapeKind::Circle => canvas.fill_ellipse_row((xf, yf, wf, hf), bg),
            ShapeKind::Raster { w: sw, h: sh, rgba } => {
                // Straight-alpha sprite blit, nearest-sampled onto the box
                // (sprites are baked at the tile sizes, so this is normally
                // a 1:1 row copy). `bg` is ignored — the artwork owns its
                // pixels; the box needs SOME background for this arm to run,
                // the builder sets a transparent one.
                let (sw, sh) = (*sw as i32, *sh as i32);
                if sw > 0 && sh > 0 {
                    let sy = ((canvas.y - y).clamp(0, h - 1) as i64 * sh as i64 / h as i64)
                        .clamp(0, (sh - 1) as i64) as i32;
                    let x0 = x.max(0);
                    let x1 = (x + w).min(canvas.width);
                    for px in x0..x1 {
                        let sx = ((px - x) as i64 * sw as i64 / w as i64).clamp(0, (sw - 1) as i64)
                            as i32;
                        let o = ((sy * sw + sx) * 4) as usize;
                        if o + 3 < rgba.len() && rgba[o + 3] > 0 {
                            canvas.blend(
                                px,
                                fade(nexus_layout_types::Rgba8 {
                                    r: rgba[o],
                                    g: rgba[o + 1],
                                    b: rgba[o + 2],
                                    a: rgba[o + 3],
                                }),
                            );
                        }
                    }
                }
            }
            ShapeKind::Path(ps) => canvas.fill_contour_row(ps, xf, yf, wf, hf, bg),
            ShapeKind::Vector(contours) => {
                for ps in contours {
                    canvas.fill_contour_row(ps, xf, yf, wf, hf, bg);
                }
            }
            ShapeKind::Stroke { paths, width_milli } => {
                canvas.stroke_paths_row(paths, (xf, yf, wf, hf), *width_milli, bg)
            }
        }
    }
    // The outline the border ring and the shine follow: the corner radius, or the full one of a
    // circle shape (a ring or a shine drawn by the corner radius would square a round element).
    let radius = match b.visual.shape {
        ShapeKind::Circle => w.min(h) / 2,
        _ => {
            (b.visual.corner_radius.top_left.0.max(0) as i64 * radius_pct.max(1) as i64 / 100)
                as i32
        }
    };
    // The `inset 0 1px 0` top-shine goes UNDER the border ring: the hairline
    // is the outermost pixel, the shine the one just inside it.
    let mut border = b.visual.border;
    for side in [&mut border.top, &mut border.right, &mut border.bottom, &mut border.left] {
        if let Some(edge) = side.as_mut() {
            edge.color = fade(edge.color);
        }
    }
    if let Some(shine) = b.visual.inset_highlight {
        paint_inset_highlight_row(canvas, (x, y, w, h), radius, &border, fade(shine));
    }
    paint_borders_row(canvas, x, y, w, h, radius, &border);
}

/// Whether `b` is a compositor region — a blur root, or an opaque surface at full opacity: its
/// shadow is the compositor's (drawn under the region, blurred on the GPU), never painted here.
fn is_region(b: &LayoutBox) -> bool {
    let full = b.visual.opacity.is_none_or(|o| o.0 >= 255);
    !b.glass_nested && b.visual.material.is_some_and(|m| m.blurs() || (m.opaque() && full))
}

/// How far `b`'s painted shadows reach past its own box, `(left, top, right, bottom)` in px (`0`
/// where they do not): its drop shadow as `paint_shadow_row` draws it (the offset, the spread
/// and the blur band) and its text shadows as the glyph painter blurs them
/// (`nexus_text_baked::blur_reach` around the offset — the label halo). A host that picks the
/// boxes of a row band by their rects widens each rect by it — picked by its rect alone, a box's
/// shadow ended flat at its top and bottom edge (the buttons' shadows, board cycle 2026-11-09).
/// A compositor region paints no drop shadow; text shadows are always the app's.
#[must_use]
pub fn shadow_reach(b: &LayoutBox) -> (i32, i32, i32, i32) {
    let mut r = (0, 0, 0, 0);
    let mut grow = |around: i32, ox: i32, oy: i32| {
        r = (
            r.0.max(around - ox),
            r.1.max(around - oy),
            r.2.max(around + ox),
            r.3.max(around + oy),
        );
    };
    if let Some(s) = b.visual.shadow.filter(|_| !is_region(b)) {
        grow(s.blur_radius.0.max(0) + s.spread.0, s.offset_x.0, s.offset_y.0);
    }
    for t in b.visual.text_shadows.iter().flatten() {
        let around = nexus_text_baked::blur_reach(u32::from(t.blur_radius)) as i32;
        grow(around, i32::from(t.offset_x), i32::from(t.offset_y));
    }
    r
}

/// One row of a soft drop shadow: signed distance to the (offset, spread-
/// adjusted) rounded shadow rect, alpha falls off linearly across the blur
/// band. Row-based like everything else here — no buffers, no passes.
fn paint_shadow_row(
    canvas: &mut RowCanvas<'_>,
    shadow: &nexus_layout_types::BoxShadow,
    b: &LayoutBox,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) {
    let blur = shadow.blur_radius.0.max(1) as f32;
    let sx = x + shadow.offset_x.0 - shadow.spread.0;
    let sy = y + shadow.offset_y.0 - shadow.spread.0;
    let sw = w + 2 * shadow.spread.0;
    let sh = h + 2 * shadow.spread.0;
    if sw <= 0 || sh <= 0 {
        return;
    }
    let reach = shadow.blur_radius.0;
    if canvas.y < sy - reach || canvas.y >= sy + sh + reach {
        return;
    }
    let radius = b.visual.corner_radius.top_left.0.max(0).min(sw / 2).min(sh / 2) as f32;
    let (cx, cy) = (sx as f32 + sw as f32 / 2.0, sy as f32 + sh as f32 / 2.0);
    let (hw, hh) = (sw as f32 / 2.0 - radius, sh as f32 / 2.0 - radius);
    // The KNOCKOUT shape: the element's own box, without the shadow's offset
    // or spread. CSS paints an outer shadow "as if the border box were
    // opaque" — never under the element. That is invisible under an opaque
    // fill and very visible under a translucent one, which is why it is
    // computed here rather than left to the fill to cover.
    let (kcx, kcy) = (x as f32 + w as f32 / 2.0, y as f32 + h as f32 / 2.0);
    let (khw, khh) = (w as f32 / 2.0 - radius, h as f32 / 2.0 - radius);
    let kdy = (canvas.y as f32 + 0.5 - kcy).abs() - khh;
    let kdyc = if kdy > 0.0 { kdy } else { 0.0 };
    let py = canvas.y as f32 + 0.5;
    let x0 = (sx - reach).max(0);
    let x1 = (sx + sw + reach).min(canvas.width);
    let dy = (py - cy).abs() - hh;
    let dyc = if dy > 0.0 { dy } else { 0.0 };
    for px in x0..x1 {
        let pxf = px as f32 + 0.5;
        let dx = (pxf - cx).abs() - hw;
        let dxc = if dx > 0.0 { dx } else { 0.0 };
        // Rounded-rect SDF (outside-only; interior clamps to the max axis).
        let outside = sqrt_f32(dxc * dxc + dyc * dyc);
        let inside = if dx.max(dy) < 0.0 { dx.max(dy) } else { 0.0 };
        let dist = outside + inside - radius;
        // Linear falloff across [-blur/2, +blur/2] around the rect edge.
        let t = 0.5 - dist / blur;
        if t <= 0.0 {
            continue;
        }
        let mut f = if t >= 1.0 { 1.0 } else { t };
        // Knock out the element's own footprint (1px feather at the edge).
        let kdx = (pxf - kcx).abs() - khw;
        let kdxc = if kdx > 0.0 { kdx } else { 0.0 };
        let kin = if kdx.max(kdy) < 0.0 { kdx.max(kdy) } else { 0.0 };
        let kdist = sqrt_f32(kdxc * kdxc + kdyc * kdyc) + kin - radius;
        if kdist <= 0.0 {
            continue;
        }
        if kdist < 1.0 {
            f *= kdist;
        }
        let a = (shadow.color.a as f32 * f) as u8;
        if a == 0 {
            continue;
        }
        canvas.blend(
            px,
            nexus_layout_types::Rgba8 {
                r: shadow.color.r,
                g: shadow.color.g,
                b: shadow.color.b,
                a,
            },
        );
    }
}

#[cfg(test)]
mod tests;
