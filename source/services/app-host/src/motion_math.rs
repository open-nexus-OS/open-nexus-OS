// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a `.transition` node as a motion layer the compositor moves (RFC-0109 §motion layers,
//! TASK-0354 P2) — the pure decisions: which boxes are the node's subtree (its pre-order span,
//! the numbering the layout and the painters share), how far their paint reaches at rest (drop
//! and text shadows included, cut at the surface), where a token's enter starts (its absent
//! state: an offset and transparent — a token that scales stays the app's to paint), and whether
//! a layer's root is a backdrop (one flat fill over the whole surface — a modal's scrim, drawn by
//! the compositor from one patch texel instead of a screen of store pixels). Same one-cfg shape
//! as `band_math`: the raster and the declaration are in `probe/motion_layers.rs`.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below

use animation::{AnimProp, MotionCategory, MotionToken};
use nexus_layout::LayoutBox;
use nexus_layout_types::{LayoutNode, Overflow, Position, Rgba8};

/// A rectangle `(x0, y0, x1, y1)` in surface pixels.
pub(crate) type Span = (i32, i32, i32, i32);

/// The pre-order span `[first, end)` of node `target`'s subtree in `root`'s numbering (the
/// layout's and the painters': the root is 1, a stack's in-flow children before its absolute
/// ones) — `None` when no node has that number.
pub(crate) fn subtree_span(root: &LayoutNode, target: usize) -> Option<(usize, usize)> {
    fn walk(node: &LayoutNode, index: &mut usize, target: usize) -> Option<(usize, usize)> {
        *index += 1;
        let here = *index;
        let absolute = |c: &&LayoutNode| matches!(c.item().position, Position::Absolute);
        let found = match node {
            LayoutNode::Stack(_, _, children) => {
                let mut found = None;
                for c in
                    children.iter().filter(|c| !absolute(c)).chain(children.iter().filter(absolute))
                {
                    if let Some(f) = walk(c, index, target) {
                        found = Some(f);
                        break;
                    }
                }
                found
            }
            LayoutNode::Grid(_, _, children) => {
                let mut found = None;
                for c in children {
                    if let Some(f) = walk(c, index, target) {
                        found = Some(f);
                        break;
                    }
                }
                found
            }
            _ => None,
        };
        found.or_else(|| (here == target).then_some((here, *index + 1)))
    }
    walk(root, &mut 0, target)
}

/// How far box `b`'s paint reaches past its rectangle `(left, top, right, bottom)`: its drop
/// shadow (blur, offset, spread — `nexus-scene-raster`'s shadow row), its text shadow, and a
/// pixel of anti-aliasing.
fn reach(b: &LayoutBox) -> (i32, i32, i32, i32) {
    let mut r = (1, 1, 1, 1);
    let mut grow = |l: i32, t: i32, rt: i32, bt: i32| {
        r = (r.0.max(l + 1), r.1.max(t + 1), r.2.max(rt + 1), r.3.max(bt + 1));
    };
    if let Some(s) = b.visual.shadow {
        let (blur, spread) = (s.blur_radius.0.max(0), s.spread.0);
        let (ox, oy) = (s.offset_x.0, s.offset_y.0);
        grow(blur + spread - ox, blur + spread - oy, blur + spread + ox, blur + spread + oy);
    }
    for t in b.visual.text_shadows.iter().flatten() {
        let blur = nexus_text_baked::blur_reach(u32::from(t.blur_radius)) as i32;
        let (ox, oy) = (i32::from(t.offset_x), i32::from(t.offset_y));
        grow(blur - ox, blur - oy, blur + ox, blur + oy);
    }
    r
}

/// Whether box `b` paints anything of its own (`has_text`: it carries a text run) — a fill, a
/// gradient, a shadow, a shine, a border or text — or is a part the compositor draws in its rect
/// (a glass region's blur, a scroll layer's viewport). A box that only lays its children out (a
/// column centring a 340-wide form across the whole surface) paints nothing.
fn paints(b: &LayoutBox, has_text: bool) -> bool {
    let v = &b.visual;
    let border = [v.border.top, v.border.right, v.border.bottom, v.border.left]
        .iter()
        .flatten()
        .any(|e| e.width.0 > 0 && e.color.a > 0);
    has_text
        || border
        || v.background.is_some_and(|c| c.a > 0)
        || v.background_gradient.is_some()
        || v.shadow.is_some()
        || v.inset_highlight.is_some()
        || v.material.is_some()
        || matches!(b.overflow, Overflow::Scroll(_))
}

/// Where the boxes of `span` paint at rest, cut at the surface `(w, h)`: the rectangles, with
/// their reach, of the boxes that paint (`has_text(node)`: a box carrying a text run), united —
/// the motion layer's rectangle. A scroll layer's content paints inside its container's
/// viewport, which the container's own box covers. `None`: nothing to paint.
pub(crate) fn paint_extent(
    boxes: &[LayoutBox],
    span: (usize, usize),
    size: (i32, i32),
    has_text: impl Fn(usize) -> bool,
) -> Option<Span> {
    extent(boxes, span, size, has_text, true)
}

/// The same boxes without their reach: the body a shadow is cast by — the rectangle whose
/// corner a menu grows from.
pub(crate) fn body_extent(
    boxes: &[LayoutBox],
    span: (usize, usize),
    size: (i32, i32),
    has_text: impl Fn(usize) -> bool,
) -> Option<Span> {
    extent(boxes, span, size, has_text, false)
}

fn extent(
    boxes: &[LayoutBox],
    span: (usize, usize),
    (w, h): (i32, i32),
    has_text: impl Fn(usize) -> bool,
    reaching: bool,
) -> Option<Span> {
    let mut acc: Option<Span> = None;
    for b in boxes {
        let (x, y, bw, bh) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
        if b.node_id < span.0
            || b.node_id >= span.1
            || bw <= 0
            || bh <= 0
            || b.scroll_root.is_some()
            || !paints(b, has_text(b.node_id))
        {
            continue;
        }
        let (l, t, r, bo) = if reaching { reach(b) } else { (0, 0, 0, 0) };
        let mut e = (x - l, y - t, x + bw + r, y + bh + bo);
        // A clipped box paints inside its clip (a card's `.overflow(hidden)`): its reach too.
        if let Some(c) = b.clip_rect {
            let c = (c.x.0, c.y.0, c.x.0 + c.width.0, c.y.0 + c.height.0);
            e = (e.0.max(c.0), e.1.max(c.1), e.2.min(c.2), e.3.min(c.3));
        }
        if e.2 <= e.0 || e.3 <= e.1 {
            continue;
        }
        acc = Some(match acc {
            Some(a) => (a.0.min(e.0), a.1.min(e.1), a.2.max(e.2), a.3.max(e.3)),
            None => e,
        });
    }
    let a = acc?;
    let cut = (a.0.max(0), a.1.max(0), a.2.min(w), a.3.min(h));
    (cut.2 > cut.0 && cut.3 > cut.1).then_some(cut)
}

/// The backdrop box `b` is on a surface `(w, h)`: its colour when it covers the whole surface and
/// paints nothing but one flat fill there — no gradient, border, corner, shadow, highlight, shape,
/// glass, opacity, text (`has_text`) or scroll viewport of its own (a modal's scrim).
pub(crate) fn backdrop_of(b: &LayoutBox, (w, h): (i32, i32), has_text: bool) -> Option<Rgba8> {
    let r = &b.rect;
    let covers = r.x.0 <= 0 && r.y.0 <= 0 && r.x.0 + r.width.0 >= w && r.y.0 + r.height.0 >= h;
    let v = &b.visual;
    let flat = covers
        && !has_text
        && v.opacity.is_none()
        && v.text_shadows == [None; 2]
        && b.clip_rect.is_none()
        && !matches!(b.overflow, Overflow::Scroll(_))
        && crate::layout_diff::flat_fill(v);
    v.background.filter(|c| flat && c.a > 0)
}

/// Where a `.transition` enter of `token` starts relative to its rest — `(dx, dy, opacity,
/// scale)`: the token's absent state, transparent, at its starting scale in 1/10000 (`10000`:
/// unscaled) — the compositor scales the whole layer about its centre (TASK-0354 P3). An effect
/// token (a wiggle, a pulse: it returns to rest, it does not enter) stays the app's.
pub(crate) fn enter_from(token: MotionToken) -> Option<(i16, i16, u8, u16)> {
    if token.category() == MotionCategory::Effect {
        return None;
    }
    let scale = match token.secondary_prop() {
        Some(AnimProp::ScaleX) => (token.resting(AnimProp::ScaleX, false) * 10_000.0) as u16,
        Some(_) => return None,
        None => 10_000,
    };
    let p = token.primary_prop();
    let at = token.resting(p, false);
    match p {
        AnimProp::Opacity => Some((0, 0, 0, scale)),
        AnimProp::TranslateY => Some((0, at as i16, 0, scale)),
        AnimProp::TranslateX => Some((at as i16, 0, 0, scale)),
        _ => None,
    }
}

/// On one axis, the edge of the body `[b_lo, b_hi)` a corner sits on: the one nearer the edge of
/// `[w_lo, w_hi)`; `None`: the body is centred there (within a pixel) and keeps its centre.
fn hung_edge(b_lo: i32, b_hi: i32, w_lo: i32, w_hi: i32) -> Option<i32> {
    let (lead, trail) = (b_lo - w_lo, w_hi - b_hi);
    ((lead - trail).abs() > 1).then_some(if lead < trail { b_lo } else { b_hi })
}

/// The point an enter the app paints itself scales about when its token grows from the corner
/// it hangs on: the same corner [`anchor_shift`] keeps at rest for a compositor layer — `body`'s
/// edge nearer the edge of `within` on each axis, the body's centre on an axis it is centred on.
pub(crate) fn anchor_point(body: Span, within: Span) -> (i32, i32) {
    let axis = |b_lo: i32, b_hi: i32, w_lo: i32, w_hi: i32| {
        hung_edge(b_lo, b_hi, w_lo, w_hi).unwrap_or((b_lo + b_hi) / 2)
    };
    (axis(body.0, body.2, within.0, within.2), axis(body.1, body.3, within.1, within.3))
}

/// How far an enter that grows from `scale` (1/10000) is shifted at its start so that the layer
/// resting at `layer` grows out of the corner it hangs on instead of its centre — `(dx, dy)` to
/// add to the enter's offset. The corner is `body`'s (the layer without its shadow's reach): on
/// each axis the edge nearer the edge of `within`, the box of the node that carries the
/// transition; an axis the body is centred on (within a pixel) keeps the centre. Scaling by `s`
/// about a point `o` is scaling by `s` about the centre `c` and moving by `(1 − s)(o − c)`; the
/// compositor evaluates a layer's offset and scale on one curve, so the corner rests where the
/// app laid it out through the whole motion and no origin crosses the wire.
pub(crate) fn anchor_shift(scale: u16, layer: Span, body: Span, within: Span) -> (i16, i16) {
    let gain = 10_000 - i32::from(scale);
    let axis = |lo: i32, hi: i32, b_lo: i32, b_hi: i32, w_lo: i32, w_hi: i32| -> i16 {
        let Some(edge) = hung_edge(b_lo, b_hi, w_lo, w_hi) else { return 0 };
        // Twice the distance from the layer's centre to the corner's edge (half pixels).
        let twice = 2 * edge - (lo + hi);
        let shift = gain * twice;
        ((shift + shift.signum() * 10_000) / 20_000).clamp(-512, 512) as i16
    };
    (
        axis(layer.0, layer.2, body.0, body.2, within.0, within.2),
        axis(layer.1, layer.3, body.1, body.3, within.1, within.3),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{BoxShadow, FxPx, Rect, Stack, TextShadow, VisualStyle};

    fn stack(children: Vec<LayoutNode>) -> LayoutNode {
        LayoutNode::Stack(Stack::default(), VisualStyle::default(), children)
    }

    fn overlay(children: Vec<LayoutNode>) -> LayoutNode {
        let mut s = Stack::default();
        s.item.position = Position::Absolute;
        LayoutNode::Stack(s, VisualStyle::default(), children)
    }

    /// The page `[leaf, overlay[leaf], stack[leaf, leaf]]`: numbered 1, leaf 2, stack 3 with 4
    /// and 5, then the overlay 6 with 7 — absolute children after the in-flow ones, as the layout
    /// and the painters number them.
    fn page() -> LayoutNode {
        stack(vec![
            stack(vec![]),
            overlay(vec![stack(vec![])]),
            stack(vec![stack(vec![]), stack(vec![])]),
        ])
    }

    #[test]
    fn a_subtree_is_its_pre_order_span() {
        let p = page();
        assert_eq!(subtree_span(&p, 1), Some((1, 8)), "the page is everything");
        assert_eq!(subtree_span(&p, 2), Some((2, 3)), "a leaf");
        assert_eq!(subtree_span(&p, 3), Some((3, 6)), "the in-flow stack and its two leaves");
        assert_eq!(subtree_span(&p, 6), Some((6, 8)), "the overlay, numbered last");
        assert_eq!(subtree_span(&p, 7), Some((7, 8)));
        assert_eq!(subtree_span(&p, 8), None, "no node 8");
        assert_eq!(subtree_span(&p, 0), None);
    }

    fn boxed(node_id: usize, (x, y, w, h): (i32, i32, i32, i32)) -> LayoutBox {
        LayoutBox {
            node_id,
            rect: Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h)),
            ..LayoutBox::default()
        }
    }

    /// A panel's extent: its box and its drop shadow, its children's text shadow; boxes outside
    /// the span and a scroll layer's content add nothing; the surface cuts it.
    #[test]
    fn a_layer_is_where_its_subtree_paints_at_rest() {
        let mut panel = boxed(10, (1500, 52, 330, 400));
        panel.visual.shadow = Some(BoxShadow {
            offset_x: FxPx::new(0),
            offset_y: FxPx::new(12),
            blur_radius: FxPx::new(24),
            spread: FxPx::new(0),
            color: nexus_layout_types::Rgba8::new(0, 0, 0, 90),
        });
        let mut label = boxed(11, (1510, 60, 100, 20));
        label.visual.text_shadows = [
            Some(TextShadow {
                offset_x: 0,
                offset_y: 1,
                blur_radius: 2,
                color: nexus_layout_types::Rgba8::new(0, 0, 0, 90),
            }),
            None,
        ];
        let mut row = boxed(12, (1510, 5000, 300, 40));
        row.scroll_root = Some(11);
        row.visual.background = Some(nexus_layout_types::Rgba8::new(9, 9, 9, 255));
        let outside = boxed(9, (0, 0, 1920, 36));
        // A column laying the panel out across the whole surface paints nothing itself.
        let column = boxed(13, (0, 40, 1920, 600));
        let boxes = [outside, panel, label, row, column];
        let text = |id: usize| id == 11;
        let e = paint_extent(&boxes, (10, 14), (1920, 1080), text).expect("paints");
        assert_eq!(e, (1500 - 25, 52 - 13, 1830 + 25, 452 + 37));
        // Cut at the surface's right edge.
        let mut wide = boxes.clone();
        wide[1].rect.x = FxPx::new(1700);
        assert_eq!(paint_extent(&wide, (10, 11), (1920, 1080), text).map(|e| e.2), Some(1920));
        assert_eq!(paint_extent(&boxes, (20, 30), (1920, 1080), text), None, "nothing there");
        assert_eq!(paint_extent(&boxes, (13, 14), (1920, 1080), text), None, "a bare column");
    }

    /// A modal's root — one flat fill over the whole surface — is a backdrop; a fill that leaves
    /// an edge bare, has a corner, a border, an opacity or text is not.
    #[test]
    fn test_reject_a_backdrop_that_is_more_than_one_flat_fill() {
        let scrim = nexus_layout_types::Rgba8::new(0, 0, 0, 102);
        let mut root = boxed(30, (0, 0, 1920, 1080));
        root.visual.background = Some(scrim);
        let surface = (1920, 1080);
        assert_eq!(backdrop_of(&root, surface, false), Some(scrim));
        assert_eq!(backdrop_of(&root, surface, true), None, "it carries text");
        let mut short = root.clone();
        short.rect.height = FxPx::new(1000);
        assert_eq!(backdrop_of(&short, surface, false), None, "an edge left bare");
        let mut round = root.clone();
        round.visual.corner_radius = nexus_layout_types::CornerRadius::uniform(FxPx::new(12));
        assert_eq!(backdrop_of(&round, surface, false), None, "a corner");
        let mut faded = root.clone();
        faded.visual.opacity = Some(nexus_layout_types::Fraction::new(128));
        assert_eq!(backdrop_of(&faded, surface, false), None, "an opacity of its own");
        let mut clear = root.clone();
        clear.visual.background = Some(nexus_layout_types::Rgba8::new(0, 0, 0, 0));
        assert_eq!(backdrop_of(&clear, surface, false), None, "a transparent fill");
        assert_eq!(backdrop_of(&boxed(30, (0, 0, 1920, 1080)), surface, false), None, "no fill");
    }

    /// TASK-0354 P2/P3: the enters the compositor draws start where the design draws them — a
    /// slide from its travel at 0.97/0.96, a dialog 10 px low at 0.94, the toast from the leading
    /// edge at 0.96, a fade from transparent — and scale as one layer; an effect token (it
    /// returns to rest) stays the app's.
    #[test]
    fn test_reject_an_effect_token_as_a_motion_layer() {
        assert_eq!(enter_from(MotionToken::SlideUp), Some((0, 16, 0, 9_700)));
        assert_eq!(enter_from(MotionToken::SlideDown), Some((0, -10, 0, 9_600)));
        assert_eq!(enter_from(MotionToken::FadeScale), Some((0, 10, 0, 9_400)));
        assert_eq!(enter_from(MotionToken::PushInline), Some((-16, 0, 0, 9_600)));
        assert_eq!(enter_from(MotionToken::Fade), Some((0, 0, 0, 10_000)));
        assert_eq!(enter_from(MotionToken::Pulse), None, "an effect: the app plays it");
        assert_eq!(enter_from(MotionToken::Wiggle), None);
        assert_eq!(enter_from(MotionToken::Snappy), Some((0, 0, 0, 10_000)), "enters as a fade");
        assert_eq!(enter_from(MotionToken::MenuIn), Some((0, 0, 0, 9_200)), "a menu grows in");
    }

    /// TASK-0354 P11: a menu grows out of the corner it hangs on. A 240×300 panel under the
    /// window's top-left (12 px in), its shadow reaching 20 px: at 0.92 the layer starts shifted
    /// so that the panel's top-left corner is where it rests — and stays there at every scale.
    #[test]
    fn a_menu_grows_from_the_corner_it_hangs_on() {
        let within = (0, 0, 900, 600);
        let body = (12, 40, 252, 340);
        let layer = (0, 20, 272, 360);
        let (dx, dy) = anchor_shift(9_200, layer, body, within);
        assert_eq!((dx, dy), (-10, -12));
        // The corner's place at scale s with the offset scaled along: c + s(o − c) + d(1 − e),
        // where s = s0 + (1 − s0)e — within a pixel of its rest at every step.
        let (cx, cy) = ((layer.0 + layer.2) as f32 / 2.0, (layer.1 + layer.3) as f32 / 2.0);
        for step in 0..=10 {
            let e = step as f32 / 10.0;
            let s = 0.92 + 0.08 * e;
            let x = cx + s * (body.0 as f32 - cx) + f32::from(dx) * (1.0 - e);
            let y = cy + s * (body.1 as f32 - cy) + f32::from(dy) * (1.0 - e);
            assert!((x - body.0 as f32).abs() < 1.0 && (y - body.1 as f32).abs() < 1.0, "{e}");
        }
        // Hung on the trailing side (the app menu, 145 px from the right edge): the other corner.
        let trailing = (515, 40, 755, 340);
        let (dx, _) = anchor_shift(9_200, (495, 20, 775, 360), trailing, within);
        assert_eq!(dx, 10);
        // Centred on both axes (a context menu in the middle): it grows from its centre.
        assert_eq!(anchor_shift(9_200, (310, 130, 590, 470), (330, 150, 570, 450), within), (0, 0));
        // The app's own enter scales about the very corner the layer's shift keeps at rest.
        assert_eq!(anchor_point(body, within), (12, 40));
        assert_eq!(anchor_point(trailing, within), (755, 40));
        assert_eq!(anchor_point((330, 150, 570, 450), within), (450, 300));
        // An unscaled enter has no corner to keep.
        assert_eq!(anchor_shift(10_000, layer, body, within), (0, 0));
    }

    /// The body is the layer without its shadow's reach.
    #[test]
    fn the_body_leaves_the_shadow_out() {
        let mut panel = boxed(2, (12, 40, 240, 300));
        panel.visual.background = Some(Rgba8::new(255, 255, 255, 255));
        panel.visual.shadow = Some(BoxShadow {
            offset_x: FxPx::new(0),
            offset_y: FxPx::new(8),
            blur_radius: FxPx::new(24),
            spread: FxPx::new(0),
            ..BoxShadow::default()
        });
        let boxes = [boxed(1, (0, 0, 900, 600)), panel];
        let body = body_extent(&boxes, (1, 3), (900, 600), |_| false);
        assert_eq!(body, Some((12, 40, 252, 340)));
        let layer = paint_extent(&boxes, (1, 3), (900, 600), |_| false).unwrap();
        assert!(layer.0 < 12 && layer.3 > 340, "the layer reaches past the body: {layer:?}");
    }
}
