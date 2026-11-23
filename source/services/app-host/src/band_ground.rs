// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ground a scroll band is rastered on (TASK-0351 P14, RFC-0109): a band's rows
//! are painted from transparent, so the compositor blends every slice over the pane below it —
//! and draws the pane under it for nothing. When the viewport lies on one flat opaque box (the
//! design's `surface-pane`, or the container's own fill) with nothing painted between, the
//! band starts from that box's pixel and is declared `BAND_OPAQUE`: windowd composes the slice
//! opaque, the planner cuts the pane, the body, the glass back and the wallpaper under it. The
//! rule is conservative: a rounded corner, a border, a gradient, a translucent fill or a box
//! painted in between keeps the band transparent (a wrong ground would show as a flat patch).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (host); `nexus-gfx::tests::frame_window_cost` for what the
//!   compositor draws for an opaque slice; the board's `gpud: gpu work` straight pixels

use nexus_layout::LayoutBox;
use nexus_layout_types::{Rgba8, ShapeKind};

extern crate alloc;

/// A viewport in layout coordinates: x0, y0, x1, y1 (exclusive).
type Rect = (i32, i32, i32, i32);

/// The room a viewport keeps from the ground box's edge: its widest border (painted over the
/// fill along the edge), none for a box without one.
fn inset(b: &LayoutBox) -> i32 {
    let bd = &b.visual.border;
    [bd.top, bd.right, bd.bottom, bd.left]
        .iter()
        .flatten()
        .map(|s| s.width.0)
        .max()
        .unwrap_or(0)
        .max(0)
}

/// The flat opaque pixel the band of the scroll container at `container` starts from: the
/// container's own fill, else the nearest box below it (in paint order) that paints under the
/// viewport — only when that box covers the viewport inside its straight cross. `None` keeps
/// the band transparent.
pub(crate) fn ground(boxes: &[LayoutBox], container: usize, viewport: Rect) -> Option<Rgba8> {
    let c = boxes.get(container)?;
    if let Some(bg) = flat_opaque(c).filter(|_| covers(c, viewport)) {
        return Some(bg);
    }
    for b in boxes[..container].iter().rev() {
        // Another layer's content paints in its own band, never under this one.
        if b.scroll_root.is_some() || !paints(b) || !intersects(b, viewport) {
            continue;
        }
        return flat_opaque(b).filter(|_| covers(b, viewport));
    }
    None
}

/// Why a band's ground is what it is (TASK-0351 P14 — the board's scroll blocks blended the
/// region and the slice at the same pixel count, so BAND_OPAQUE did not take effect, and no line
/// said why): `yes` a flat opaque ground; `no_pane` nothing paints under the viewport; `corner` the
/// box under it is rounded or does not cover the viewport inside its straight cross; `translucent`
/// its fill is see-through, a gradient or a shine; `other` it paints no fill (a border alone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Why {
    Yes,
    NoPane,
    Corner,
    Translucent,
    Other,
}

/// The reason `ground` gives for the scroll container at `container` (its same walk).
pub(crate) fn why(boxes: &[LayoutBox], container: usize, viewport: Rect) -> Why {
    if ground(boxes, container, viewport).is_some() {
        return Why::Yes;
    }
    let Some(c) = boxes.get(container) else { return Why::Other };
    let under = boxes[..container]
        .iter()
        .rev()
        .find(|b| b.scroll_root.is_none() && paints(b) && intersects(b, viewport));
    let Some(b) = Some(c).filter(|c| paints(c)).or(under) else { return Why::NoPane };
    let v = &b.visual;
    if v.background_gradient.is_some()
        || v.inset_highlight.is_some()
        || v.opacity.is_some_and(|o| o.0 < 255)
        || v.background.is_some_and(|c| c.a < 255)
    {
        Why::Translucent
    } else if v.background.is_none() {
        Why::Other
    } else {
        Why::Corner
    }
}

/// The reasons counted over the band grounds that changed, and the line once 16 more came:
/// `apphost: band opaque (yes= no_pane= corner= translucent= other=)`.
#[cfg(nexus_env = "os")]
pub(crate) fn note(w: Why) -> Option<alloc::string::String> {
    use core::sync::atomic::{AtomicU32, Ordering};
    static SEEN: [AtomicU32; 5] = [const { AtomicU32::new(0) }; 5];
    SEEN[w as usize].fetch_add(1, Ordering::Relaxed);
    let n: [u32; 5] = core::array::from_fn(|i| SEEN[i].load(Ordering::Relaxed));
    (n.iter().sum::<u32>() % 16 == 0).then(|| {
        alloc::format!(
            "apphost: band opaque (yes={} no_pane={} corner={} translucent={} other={})",
            n[0],
            n[1],
            n[2],
            n[3],
            n[4]
        )
    })
}

/// Whether the box puts any pixel on the base.
fn paints(b: &LayoutBox) -> bool {
    let bd = &b.visual.border;
    b.visual.background.is_some()
        || b.visual.background_gradient.is_some()
        || b.visual.inset_highlight.is_some()
        || bd.top.is_some()
        || bd.right.is_some()
        || bd.bottom.is_some()
        || bd.left.is_some()
}

/// The box's one flat opaque colour: a rectangle at full opacity with an opaque fill, no
/// gradient and no shine.
fn flat_opaque(b: &LayoutBox) -> Option<Rgba8> {
    let v = &b.visual;
    if !matches!(v.shape, ShapeKind::Rect)
        || v.background_gradient.is_some()
        || v.inset_highlight.is_some()
        || v.opacity.is_some_and(|o| o.0 < 255)
    {
        return None;
    }
    v.background.filter(|c| c.a == 255)
}

/// The box's painted rectangle (its rect within its clip).
fn painted(b: &LayoutBox) -> Rect {
    let r = b.rect;
    let mut p = (r.x.0, r.y.0, r.x.0 + r.width.0, r.y.0 + r.height.0);
    if let Some(c) = b.clip_rect {
        p = (
            p.0.max(c.x.0),
            p.1.max(c.y.0),
            p.2.min(c.x.0 + c.width.0),
            p.3.min(c.y.0 + c.height.0),
        );
    }
    p
}

fn intersects(b: &LayoutBox, v: Rect) -> bool {
    let p = painted(b);
    p.0 < v.2 && v.0 < p.2 && p.1 < v.3 && v.1 < p.3
}

fn inside(v: Rect, r: Rect) -> bool {
    r.0 < r.2 && r.1 < r.3 && v.0 >= r.0 && v.1 >= r.1 && v.2 <= r.2 && v.3 <= r.3
}

/// Whether the viewport lies inside the box's straight cross (the rounded rectangle at
/// coverage one), its border's width from its edge.
fn covers(b: &LayoutBox, v: Rect) -> bool {
    let (x0, y0, x1, y1) = painted(b);
    let cr = &b.visual.corner_radius;
    let r = cr.top_left.0.max(cr.top_right.0).max(cr.bottom_left.0).max(cr.bottom_right.0).max(0);
    let i = inset(b);
    let column = (x0 + r + i, y0 + i, x1 - r - i, y1 - i);
    let row = (x0 + i, y0 + r + i, x1 - i, y1 - r - i);
    inside(v, column) || inside(v, row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{FxPx, Rect as LRect};

    const PANE: Rgba8 = Rgba8 { r: 0x2c, g: 0x31, b: 0x3d, a: 255 };

    fn boxed(x: i32, y: i32, w: i32, h: i32, bg: Option<Rgba8>, radius: i32) -> LayoutBox {
        let mut b = LayoutBox {
            rect: LRect { x: FxPx(x), y: FxPx(y), width: FxPx(w), height: FxPx(h) },
            ..LayoutBox::default()
        };
        b.visual.background = bg;
        b.visual.corner_radius.top_left = FxPx(radius);
        b.visual.corner_radius.top_right = FxPx(radius);
        b.visual.corner_radius.bottom_left = FxPx(radius);
        b.visual.corner_radius.bottom_right = FxPx(radius);
        b
    }

    /// A window body, its opaque pane (300, 200, 600×480, radius 16) and a list in it.
    fn page() -> Vec<LayoutBox> {
        vec![
            boxed(0, 0, 1000, 800, Some(Rgba8 { r: 40, g: 40, b: 40, a: 200 }), 24),
            boxed(300, 200, 600, 480, Some(PANE), 16),
            boxed(320, 220, 560, 440, None, 0),
        ]
    }

    #[test]
    fn a_band_on_an_opaque_pane_starts_from_the_panes_pixel() {
        assert_eq!(ground(&page(), 2, (320, 220, 880, 660)), Some(PANE));
    }

    #[test]
    fn the_containers_own_opaque_fill_is_its_ground() {
        let mut p = page();
        let fill = Rgba8 { r: 1, g: 2, b: 3, a: 255 };
        p[2].visual.background = Some(fill);
        assert_eq!(ground(&p, 2, (320, 220, 880, 660)), Some(fill));
    }

    #[test]
    fn test_reject_a_band_touching_the_panes_corner() {
        // Flush to the pane's edges on both axes: a corner pixel is not covered.
        assert_eq!(ground(&page(), 2, (300, 200, 900, 680)), None);
        // Flush left and right, the rows inside the corners: the cross's row covers it.
        assert_eq!(ground(&page(), 2, (300, 220, 900, 660)), Some(PANE));
    }

    #[test]
    fn test_reject_a_band_over_the_panes_border() {
        let mut p = page();
        let ink = Rgba8 { r: 0, g: 0, b: 0, a: 18 };
        p[1].visual.border = nexus_layout_types::EdgeBorder::all(FxPx(1), ink);
        assert_eq!(ground(&p, 2, (300, 220, 900, 660)), None, "over the hairline");
        assert_eq!(ground(&p, 2, (301, 220, 899, 660)), Some(PANE), "inside it");
    }

    #[test]
    fn test_reject_a_translucent_or_graded_pane() {
        let mut p = page();
        p[1].visual.background = Some(Rgba8 { a: 200, ..PANE });
        assert_eq!(ground(&p, 2, (320, 220, 880, 660)), None);
        let mut p = page();
        p[1].visual.background_gradient = Some((PANE, PANE));
        assert_eq!(ground(&p, 2, (320, 220, 880, 660)), None);
        let mut p = page();
        p[1].visual.opacity = Some(nexus_layout_types::Fraction(128));
        assert_eq!(ground(&p, 2, (320, 220, 880, 660)), None);
    }

    #[test]
    fn test_reject_a_box_painted_between_the_pane_and_the_band() {
        let mut p = page();
        let card = boxed(400, 300, 100, 100, Some(Rgba8 { r: 9, g: 9, b: 9, a: 90 }), 8);
        p.insert(2, card);
        assert_eq!(ground(&p, 3, (320, 220, 880, 660)), None);
        // The same card beside the viewport leaves the ground alone.
        p[2].rect.x = FxPx(0);
        assert_eq!(ground(&p, 3, (320, 220, 880, 660)), Some(PANE));
    }

    #[test]
    fn a_band_over_the_translucent_window_alone_stays_transparent() {
        assert_eq!(ground(&page(), 2, (20, 20, 200, 100)), None);
    }

    #[test]
    fn a_band_ground_names_why_it_is_not_opaque() {
        let pane = boxed(0, 0, 400, 600, Some(PANE), 0);
        let viewport = (20, 20, 380, 580);
        assert_eq!(why(&[pane.clone(), boxed(20, 20, 360, 560, None, 0)], 1, viewport), Why::Yes);
        assert_eq!(why(&[boxed(20, 20, 360, 560, None, 0)], 0, viewport), Why::NoPane);
        let glass = boxed(0, 0, 400, 600, Some(Rgba8 { a: 180, ..PANE }), 0);
        assert_eq!(why(&[glass, boxed(20, 20, 360, 560, None, 0)], 1, viewport), Why::Translucent);
        let rounded = boxed(0, 0, 400, 600, Some(PANE), 40);
        assert_eq!(
            why(&[rounded, boxed(0, 0, 400, 600, None, 0)], 1, (0, 0, 400, 600)),
            Why::Corner
        );
    }
}
