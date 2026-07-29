// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The SCROLLED row painters — the paint-time scroll transform over
//! the retained boxes (pretext: scrolling is a repaint with an offset, never
//! a re-layout), the picked/animated row loops, and the per-box viewport
//! scissor logic that mirrors `interact::hit_scrolled` in the DSL runtime.
//! Split out of `lib.rs` (module-size ratchet): the shape painters stay
//! there, the scroll/pick orchestration lives here.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: golden tests in `tests/` + dsl_apps_conformance drive these
//! through the app-host painters.

use crate::anim;
use crate::{paint_box_row, HoverWash, NodeAnim, RowCanvas};
use nexus_layout::LayoutBox;
use nexus_layout_types::Rgba8;

/// Paint-time scroll transform for the page's scroll viewport (pretext:
/// scrolling is a REPAINT with an offset over the RETAINED boxes — never a
/// re-layout, never a per-event allocation). Boxes carrying a `clip_rect`
/// (the engine stamps it on every descendant of an `Overflow::Hidden`
/// container) render shifted by `(dx, dy)` and scissored to the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollView {
    /// Viewport rect on the surface: x0, y0, x1, y1 (exclusive).
    pub clip: (i32, i32, i32, i32),
    /// Content shift right→left (horizontal scroll offset).
    pub dx: i32,
    /// Content shift down→up (vertical scroll offset).
    pub dy: i32,
}

/// [`paint_row`] plus an optional hover wash over the hovered box. The wash
/// paints directly after its box (before later siblings/children), so nested
/// content still reads on top of the wash like a real material highlight.
pub fn paint_row_hover(canvas: &mut RowCanvas<'_>, boxes: &[LayoutBox], hover: Option<HoverWash>) {
    paint_row_scrolled(canvas, boxes, hover, None);
}

/// [`paint_row_hover`] with the scroll transform. `canvas.y` is the SURFACE
/// row; clipped boxes are sampled at model row `canvas.y + dy` and shifted
/// left by `dx`, unclipped boxes paint identity — one pass, alloc-free.
pub fn paint_row_scrolled(
    canvas: &mut RowCanvas<'_>,
    boxes: &[LayoutBox],
    hover: Option<HoverWash>,
    scroll: Option<ScrollView>,
) {
    let surface_y = canvas.y;
    for b in boxes {
        paint_one_scrolled(canvas, b, hover, scroll, surface_y, None);
    }
}

/// [`paint_row_scrolled`] over a PRE-FILTERED index list (`pick` = indices
/// into `boxes` that intersect the repaint span). The caller computes the
/// visibility set ONCE per repaint — the per-row cost is then proportional
/// to what is on screen, not to the page's total box count (the 1000-message
/// transcript contract). Alloc-free.
pub fn paint_row_picked(
    canvas: &mut RowCanvas<'_>,
    boxes: &[LayoutBox],
    pick: &[u32],
    hover: Option<HoverWash>,
    scroll: Option<ScrollView>,
) {
    paint_row_picked_animated(canvas, boxes, pick, hover, scroll, &[]);
}

/// [`paint_row_picked`] with per-node **animation** transforms (opacity fade +
/// translate + uniform scale, keyed by `node_id`) applied to matching boxes —
/// the paint tail of the DSL `.animate`/`.transition`/`.effect` binding
/// (docs/dev/ui/foundations/animation.md). An identity/absent `NodeAnim`
/// paints exactly as [`paint_row_picked`]. Alloc-free; `anims` is bounded by
/// the host's active-animation cap.
pub fn paint_row_picked_animated(
    canvas: &mut RowCanvas<'_>,
    boxes: &[LayoutBox],
    pick: &[u32],
    hover: Option<HoverWash>,
    scroll: Option<ScrollView>,
    anims: &[NodeAnim],
) {
    let surface_y = canvas.y;
    for &i in pick {
        let Some(b) = boxes.get(i as usize) else { continue };
        let anim = anims.iter().find(|a| a.node_id == b.node_id && !a.is_identity());
        paint_one_scrolled(canvas, b, hover, scroll, surface_y, anim);
    }
}

/// [`paint_row_picked_animated`] with a PRECOMPUTED per-picked-box animation
/// index (`anim_of[k]` = index into `anims` for `pick[k]`, `-1` = none) — the
/// caller resolves the box→anim mapping ONCE per repaint instead of the
/// painter scanning the anims slice per box per row. With the interaction
/// subtree cascade (up to ~48 entries) the per-row scan multiplied into
/// millions of comparisons per shell repaint ("hover makes everything slow").
pub fn paint_row_picked_indexed(
    canvas: &mut RowCanvas<'_>,
    boxes: &[LayoutBox],
    pick: &[u32],
    anim_of: &[i16],
    hover: Option<HoverWash>,
    scroll: Option<ScrollView>,
    anims: &[NodeAnim],
) {
    let surface_y = canvas.y;
    for (k, &i) in pick.iter().enumerate() {
        let Some(b) = boxes.get(i as usize) else { continue };
        let anim = anim_of
            .get(k)
            .and_then(|&ai| if ai >= 0 { anims.get(ai as usize) } else { None })
            .filter(|a| !a.is_identity());
        paint_one_scrolled(canvas, b, hover, scroll, surface_y, anim);
    }
}

#[inline]
fn paint_one_scrolled(
    canvas: &mut RowCanvas<'_>,
    b: &LayoutBox,
    hover: Option<HoverWash>,
    scroll: Option<ScrollView>,
    surface_y: i32,
    anim: Option<&NodeAnim>,
) {
    {
        // A clipped box scissors to its OWN viewport; the scroll transform
        // applies only when that clip lies inside the ACTIVE container's
        // viewport (mirrors `interact::hit_scrolled` — pixels and input must
        // agree). Three cases: direct scroll content (clip == viewport),
        // a nested clip riding the scrolled content (window itself shifts),
        // and a DIFFERENT static viewport (second `.scroll` container or a
        // widget-internal clip) which paints identity in its own window —
        // that last case used to be dragged along by the active transform.
        let scrolled = match (scroll, b.clip_rect) {
            (Some(sv), Some(clip)) => {
                let (cx0, cy0) = (clip.x.0, clip.y.0);
                let (cx1, cy1) = (cx0 + clip.width.0, cy0 + clip.height.0);
                let vc = sv.clip;
                if (cx0, cy0, cx1, cy1) == vc {
                    // Direct scroll content: visible on the viewport's rows,
                    // sampled at the shifted model row.
                    if surface_y < vc.1 || surface_y >= vc.3 {
                        return;
                    }
                    canvas.y = surface_y + sv.dy;
                    canvas.shift_x = sv.dx;
                    canvas.clip_x = Some((vc.0, vc.2));
                    true
                } else if cx0 >= vc.0 && cy0 >= vc.1 && cx1 <= vc.2 && cy1 <= vc.3 {
                    // Nested clip inside the scrolled content: its window is
                    // shifted with the content, bounded by the viewport.
                    let y0 = (cy0 - sv.dy).max(vc.1);
                    let y1 = (cy1 - sv.dy).min(vc.3);
                    if surface_y < y0 || surface_y >= y1 {
                        return;
                    }
                    canvas.y = surface_y + sv.dy;
                    canvas.shift_x = sv.dx;
                    canvas.clip_x = Some(((cx0 - sv.dx).max(vc.0), (cx1 - sv.dx).min(vc.2)));
                    true
                } else {
                    // A different, static viewport: identity paint scissored
                    // to its own window.
                    if surface_y < cy0 || surface_y >= cy1 {
                        return;
                    }
                    canvas.clip_x = Some((cx0, cx1));
                    true
                }
            }
            _ => false,
        };
        // Per-node animation transform (opacity fade + translate + uniform
        // scale). A matching non-identity `NodeAnim` replaces the box's fill
        // with a transformed, alpha-scaled draw; otherwise the box paints as
        // usual. Text is faded/translated by the caller's glyph pass.
        match anim {
            Some(a) => anim::paint_anim_box_row(canvas, b, a),
            None => paint_box_row(canvas, b),
        }
        if let Some(hw) = hover {
            if b.node_id == hw.node_id && (hw.color.a > 0 || hw.ring_alpha > 0) {
                let (bx, by, bw, bh) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
                // Track the hover-grow: wash + ring follow the ANIMATED rect.
                let (x, y, w, h, rpct) = match anim {
                    Some(a) => {
                        let (nx, ny, nw, nh) = a.transform_rect(bx, by, bw, bh);
                        (nx, ny, nw, nh, a.radius_pct())
                    }
                    None => (bx, by, bw, bh, 100),
                };
                if w > 0 && h > 0 && canvas.y >= y && canvas.y < y + h {
                    let radius = (b.visual.corner_radius.top_left.0.max(0) as i64
                        * rpct.max(1) as i64
                        / 100) as i32;
                    if hw.color.a > 0 {
                        canvas.fill_round_rect_row(x, y, w, h, radius, hw.color);
                    }
                    if hw.ring_alpha > 0 {
                        // Bright 2px outline (reads as the Tahoe hover ring on
                        // both themes, over the wash).
                        let ring = Rgba8::new(255, 255, 255, hw.ring_alpha);
                        let inside = canvas.y >= y + 2 && canvas.y < y + h - 2;
                        if !inside {
                            canvas.fill_round_rect_row(x, y, w, h, radius, ring);
                        } else {
                            canvas.fill_round_rect_row(x, y, 2, h, 0, ring);
                            canvas.fill_round_rect_row(x + w - 2, y, 2, h, 0, ring);
                        }
                    }
                }
            }
        }
        if scrolled {
            canvas.y = surface_y;
            canvas.shift_x = 0;
            canvas.clip_x = None;
        }
    }
}
