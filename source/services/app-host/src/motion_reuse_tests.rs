// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The store-reuse decisions on the host (package B 1a): a changed subtree is never kept, a
//! compaction and a theme drop what was parked, a reopen of the same subtree paints nothing, a
//! parked store gives way before a ghost. The scene below (a panel in a layout, the motion pass's
//! row painter) is shared with the 200-sequence oracle (`motion_reuse_oracle.rs`).

use super::*;
use crate::layout_diff::hides;
use crate::motion_math::paint_extent;
use crate::pass_plan::PassPlan;
use crate::store_alloc::{Item, StoreAlloc};
use nexus_layout_types::{
    BoxShadow, CornerRadius, FxPx, Rect, Rgba8, TextAlign, TextStyle, VisualStyle,
};
use nexus_text_baked::measure_text::BakedTextMeasure;
use std::vec::Vec;

pub(super) const W: i32 = 320;
pub(super) const H: i32 = 240;
/// The probe's store keys (`probe/store.rs`).
pub(super) const LAYER_KEY: u32 = 1 << 17;
pub(super) const PARKED_KEY: u32 = 1 << 19;
/// The transition's identity (one `.transition` node carries the panel).
pub(super) const IDENT: i32 = 7;

fn rect((x, y, w, h): (i32, i32, i32, i32)) -> Rect {
    Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h))
}

fn boxed(at: (i32, i32, i32, i32), visual: VisualStyle) -> LayoutBox {
    LayoutBox { rect: rect(at), visual, ..LayoutBox::default() }
}

fn shadow(y: i32, blur: i32, a: u8) -> Option<BoxShadow> {
    Some(BoxShadow {
        offset_x: FxPx::new(0),
        offset_y: FxPx::new(y),
        blur_radius: FxPx::new(blur),
        spread: FxPx::new(0),
        color: Rgba8::new(0, 0, 0, a),
    })
}

fn run(content: &str, align: TextAlign) -> TextRun {
    let style = TextStyle { text_align: align, ..TextStyle::default() };
    TextRun {
        node: 0,
        content: content.into(),
        font: BakedTextMeasure::font(&style),
        color: [240, 240, 245, 255],
        weight: style.font_weight,
        tracking: BakedTextMeasure::tracking(&style),
        line: BakedTextMeasure::line_box(&style) as i32,
        align: style.text_align,
    }
}

fn flat(c: Rgba8) -> VisualStyle {
    VisualStyle { background: Some(c), ..VisualStyle::default() }
}

/// A node of the scene: its box and its run.
#[derive(Clone)]
pub(super) struct Node {
    b: LayoutBox,
    text: Option<TextRun>,
}

/// A layout as the probe retains it: boxes in pre-order (`boxes[id - 1]`), runs sorted by node.
#[derive(Clone)]
pub(super) struct Doc {
    pub(super) boxes: Vec<LayoutBox>,
    pub(super) texts: Vec<TextRun>,
}

/// The nodes before the panel, then the panel (its first node is the transition's root): the
/// layout and the panel's span.
pub(super) fn build(prefix: &[Node], panel: &[Node]) -> (Doc, (usize, usize)) {
    let mut doc = Doc { boxes: Vec::new(), texts: Vec::new() };
    for (i, n) in prefix.iter().chain(panel).enumerate() {
        let mut b = n.b.clone();
        b.node_id = i + 1;
        if let Some(t) = &n.text {
            doc.texts.push(TextRun { node: i + 1, ..t.clone() });
        }
        doc.boxes.push(b);
    }
    let first = prefix.len() + 1;
    (doc, (first, first + panel.len()))
}

fn has_text(doc: &Doc, id: usize) -> bool {
    doc.texts.binary_search_by_key(&id, |t| t.node).is_ok()
}

/// Where the subtree paints at rest (`motion_math::paint_extent`, as `layer_shape`).
pub(super) fn layer_rect(doc: &Doc, span: (usize, usize)) -> Option<Span> {
    paint_extent(&doc.boxes, span, (W, H), |id| has_text(doc, id))
}

/// A rest's size in pixels.
pub(super) fn size(r: Span) -> (u32, u32) {
    ((r.2 - r.0) as u32, (r.3 - r.1) as u32)
}

/// Rows `[y0, y1)` of the subtree `span` resting at `r`, as the motion pass paints them
/// (`probe/paint.rs::render_motion_rows`): from transparent, the canvas shifted by the rect's
/// left edge, its boxes and the runs no later glass root of the same pass hides.
pub(super) fn paint_rows(
    doc: &Doc,
    span: (usize, usize),
    r: Span,
    rows: (i32, i32),
) -> Vec<(i32, Vec<u8>)> {
    let inside = |b: &LayoutBox| b.node_id >= span.0 && b.node_id < span.1;
    let mut picks = Vec::new();
    let mut runs = Vec::new();
    for (bi, b) in doc.boxes.iter().enumerate() {
        if !inside(b) || b.rect.width.0 <= 0 || b.rect.height.0 <= 0 {
            continue;
        }
        picks.push(bi as u32);
        let ti = doc.texts.binary_search_by_key(&b.node_id, |t| t.node).ok();
        let hidden = doc.boxes.iter().any(|g| inside(g) && hides(g, b));
        if let (false, Some(ti)) = (hidden, ti) {
            runs.push((bi as u32, ti as u32));
        }
    }
    let none = vec![-1; picks.len()];
    let plan = PassPlan {
        width: W,
        boxes: &doc.boxes,
        picks: &picks,
        pick_anims: &none,
        runs: &runs,
        texts: &doc.texts,
        anims: &[],
        caret: None,
        views: &[],
    };
    let span_bytes = (r.2 - r.0).max(0) as usize * 4;
    let mut row = vec![0u8; W as usize * 4];
    let mut out = Vec::new();
    for y in rows.0.max(r.1)..rows.1.min(r.3) {
        row.fill(0);
        plan.paint_row(&mut row, y, r.0);
        out.push((y, row[..span_bytes].to_vec()));
    }
    out
}

/// A small deterministic generator (xorshift): the same sequences on every run.
pub(super) struct Rng(pub(super) u64);

impl Rng {
    pub(super) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub(super) fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn within(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as usize) as i32
    }

    fn colour(&mut self) -> Rgba8 {
        let v = self.next();
        Rgba8::new(v as u8, (v >> 8) as u8, (v >> 16) as u8, 80 + ((v >> 24) as u8 % 176))
    }
}

const WORDS: [&str; 5] = ["Wi-Fi", "Bluetooth is on", "12:41", "Aus", "An"];

/// The panel's root: a card the children stay inside (so its rest mostly keeps its rect).
const ROOT: (i32, i32, i32, i32) = (40, 30, 220, 170);

fn inside_x(x: i32, w: i32) -> i32 {
    x.clamp(ROOT.0 + 4, ROOT.0 + ROOT.2 - w - 4)
}

fn inside_y(y: i32, h: i32) -> i32 {
    y.clamp(ROOT.1 + 4, ROOT.1 + ROOT.3 - h - 4)
}

fn child(rng: &mut Rng) -> Node {
    let (w, h) = (rng.within(10, 90), rng.within(8, 40));
    let at = (inside_x(rng.within(0, W), w), inside_y(rng.within(0, H), h), w, h);
    let text = (rng.below(2) == 0).then(|| run(WORDS[rng.below(WORDS.len())], TextAlign::Left));
    let visual = match rng.below(3) {
        0 => flat(rng.colour()),
        1 => VisualStyle {
            background: Some(rng.colour()),
            corner_radius: CornerRadius::uniform(FxPx::new(6)),
            ..VisualStyle::default()
        },
        _ => VisualStyle::default(),
    };
    Node { b: boxed(at, visual), text }
}

/// The panel: a rounded card and four children inside it.
pub(super) fn panel(rng: &mut Rng) -> Vec<Node> {
    let root = VisualStyle {
        background: Some(Rgba8::new(40, 44, 52, 240)),
        corner_radius: CornerRadius::uniform(FxPx::new(12)),
        ..VisualStyle::default()
    };
    let mut nodes = vec![Node { b: boxed(ROOT, root), text: None }];
    for _ in 0..4 {
        nodes.push(child(rng));
    }
    nodes
}

/// A node before the panel (its coming and going renumbers the panel).
pub(super) fn outside(rng: &mut Rng) -> Node {
    let at = (rng.within(0, 200), rng.within(0, 20), 60, 20);
    Node { b: boxed(at, flat(rng.colour())), text: None }
}

/// One edit inside the panel (a model change while it is open, closed or parked).
pub(super) fn edit_panel(nodes: &mut Vec<Node>, rng: &mut Rng) -> &'static str {
    let k = 1 + rng.below(nodes.len() - 1);
    let r = nodes[k].b.rect;
    let (bx, by, bw, bh) = (r.x.0, r.y.0, r.width.0, r.height.0);
    match rng.below(10) {
        0 => {
            nodes[k].b.visual.background = Some(rng.colour());
            "recolour"
        }
        1 => {
            let (dx, dy) = (rng.within(-12, 12), rng.within(-12, 12));
            nodes[k].b.rect = rect((inside_x(bx + dx, bw), inside_y(by + dy, bh), bw, bh));
            "move"
        }
        2 => {
            let w = (bw + rng.within(-8, 8)).clamp(6, 90);
            let h = (bh + rng.within(-6, 6)).clamp(6, 40);
            nodes[k].b.rect = rect((inside_x(bx, w), inside_y(by, h), w, h));
            "resize"
        }
        3 => {
            nodes[k].text = match rng.below(3) {
                0 => None,
                _ => Some(run(WORDS[rng.below(WORDS.len())], TextAlign::Center)),
            };
            "text"
        }
        4 => {
            if let Some(t) = nodes[k].text.as_mut() {
                t.color = [rng.next() as u8, 200, 220, 255];
            }
            "text colour"
        }
        5 => {
            nodes[k].b.opacity = [255, 128, 64][rng.below(3)];
            "fade"
        }
        6 => {
            let s = &mut nodes[k].b.visual.shadow;
            *s = if s.is_some() { None } else { shadow(2, 4, 90) };
            "shadow"
        }
        7 => {
            let at = 1 + rng.below(nodes.len());
            let c = child(rng);
            nodes.insert(at, c);
            "insert"
        }
        8 if nodes.len() > 3 => {
            nodes.remove(k);
            "remove"
        }
        _ => {
            nodes[0].b.visual.background = Some(rng.colour());
            "recolour the root"
        }
    }
}

fn snap(doc: &Doc, span: (usize, usize), place: (u16, u16), inputs: Inputs) -> Snapshot {
    let mut s = Snapshot::default();
    let r = layer_rect(doc, span).expect("the panel paints");
    s.take((&doc.boxes, &doc.texts), span, (r, place), inputs);
    s
}

/// A store whose subtree changed is never kept: a recoloured child, a new label, a child gone,
/// an animated node, the focus inside, a moved view, another place or backdrop.
#[test]
fn test_reject_keeping_a_store_whose_subtree_changed() {
    let mut rng = Rng(11);
    let prefix = vec![outside(&mut rng)];
    let mut nodes = panel(&mut rng);
    let (doc, span) = build(&prefix, &nodes);
    let plain = || Inputs::new(0, None);
    let before = snap(&doc, span, (0, 300), plain());
    assert!(before.is_valid());
    let r = nodes[2].b.rect;
    let (y0, y1) = (r.y.0, r.y.0 + r.height.0);
    nodes[2].b.visual.background = Some(Rgba8::new(250, 10, 10, 255));
    let (doc2, span2) = build(&prefix, &nodes);
    match reuse(&before, &snap(&doc2, span2, (0, 300), plain())) {
        Reuse::Rows((a, b)) => assert!(a <= y0 && b >= y1, "rows {a}..{b} hold {y0}..{y1}"),
        r => panic!("a recoloured child: {r:?}"),
    }
    nodes[3].text = Some(run("A new label", TextAlign::Left));
    let (doc3, span3) = build(&prefix, &nodes);
    assert_ne!(reuse(&before, &snap(&doc3, span3, (0, 300), plain())), Reuse::Keep);
    nodes.remove(1);
    let (doc4, span4) = build(&prefix, &nodes);
    assert_ne!(reuse(&before, &snap(&doc4, span4, (0, 300), plain())), Reuse::Keep);
    let same = |inputs| reuse(&before, &snap(&doc, span, (0, 300), inputs));
    let animated = Inputs { animated: true, ..plain() };
    let focus = Inputs { focus: true, ..plain() };
    assert_eq!(same(animated), Reuse::Whole, "an animated node is not comparable");
    assert_eq!(same(focus), Reuse::Whole, "the caret is not comparable");
    let mut view = plain();
    view.view(2, View::Shown { clip: (40, 30, 200, 120), dx: 0, dy: 24 });
    assert_eq!(same(view), Reuse::Whole, "a moved scroll view");
    let mut band = plain();
    band.view(2, View::Band);
    assert_eq!(same(band), Reuse::Whole, "a view that became a band");
    assert_eq!(reuse(&before, &snap(&doc, span, (0, 301), plain())), Reuse::Whole);
    assert_eq!(same(Inputs::new(0, Some([0, 0, 0, 90]))), Reuse::Whole, "another backdrop");
    assert_eq!(same(plain()), Reuse::Keep, "the same subtree is kept");
    let named = [Reuse::Keep, Reuse::Rows((10, 30)), Reuse::Whole].map(|r| r.said(100));
    assert_eq!(named, [("keep", 0), ("rows", 20), ("whole", 100)], "the layer's line");
    // Boxes not dense in pre-order give no comparison.
    let mut holes = doc.clone();
    holes.boxes.remove(span.0);
    let mut s = Snapshot::default();
    s.take((&holes.boxes, &holes.texts), span, (before.rect, (0, 300)), plain());
    assert!(!s.is_valid());
}

/// A compaction moves every place: a parked store goes, and the next enter paints whole.
#[test]
fn test_reject_a_parked_store_after_a_compaction() {
    use crate::store_alloc::Outcome;
    let mut rng = Rng(5);
    let (doc, span) = build(&[], &panel(&mut rng));
    let r = layer_rect(&doc, span).expect("paints");
    let (w, h) = size(r);
    // Four quarters; the layer is the last. Its ghost parks; then a full-width item fits only
    // from scratch beside the first quarter and the parked store (the diagonal).
    let mut alloc = StoreAlloc::new(w * 2, 0, h * 2);
    let quarter = |key| Item { key, w, h };
    let quarters = [quarter(1), quarter(2), quarter(3), quarter(LAYER_KEY | 3)];
    assert_eq!(alloc.plan(&quarters), Outcome::Kept);
    let (x, y, _, _) = alloc.place_of(LAYER_KEY | 3).expect("placed");
    assert_eq!((x, y), (w, h), "the layer takes the last quarter");
    let place = (x as u16, y as u16);
    let mut cache = Cache::new();
    cache.now_mut().take((&doc.boxes, &doc.texts), span, (r, place), Inputs::new(0, None));
    cache.hold_now(0);
    let alloc_ref = &mut alloc;
    assert!(cache.park(0, (IDENT, (w, h), place), PARKED_KEY, |(x, y), key| {
        alloc_ref.rekey_at((u32::from(x), u32::from(y)), key)
    }));
    let wide = Item { key: 9, w: w * 2, h };
    let mut want = vec![wide];
    cache.parked_items(|key, w, h| want.push(Item { key, w, h }));
    want.push(quarter(1));
    let outcome = alloc.plan(&want);
    assert_eq!(outcome, Outcome::Compacted);
    assert_ne!(alloc.place_of(PARKED_KEY | 1).map(|p| (p.0, p.1)), Some((w, h)), "it moved");
    assert_eq!(give_way(outcome, (true, false, false)), GiveWay::Parked);
    assert_eq!(cache.drop_parked(), 1);
    assert_eq!(alloc.plan(&[quarter(1), wide]), Outcome::Kept);
    assert_eq!(parked_for(cache.parked(), IDENT, (w, h)), None, "nothing to take over");
    // A new layer in the slot knows nothing: whole.
    cache.forget(0);
    cache.now_mut().take((&doc.boxes, &doc.texts), span, (r, place), Inputs::new(0, None));
    assert_eq!(cache.decide(0), Reuse::Whole);
}

/// A theme change is not comparable, and a parked store of another theme goes.
#[test]
fn test_reject_reuse_across_a_theme_change() {
    let mut rng = Rng(3);
    let (doc, span) = build(&[], &panel(&mut rng));
    let dark = snap(&doc, span, (0, 0), Inputs::new(0, None));
    assert_eq!(reuse(&dark, &snap(&doc, span, (0, 0), Inputs::new(1, None))), Reuse::Whole);
    let mut cache = Cache::new();
    let r = layer_rect(&doc, span).expect("paints");
    cache.now_mut().take((&doc.boxes, &doc.texts), span, (r, (0, 0)), Inputs::new(0, None));
    cache.hold_now(1);
    assert!(cache.park(1, (IDENT, size(r), (0, 0)), PARKED_KEY, |_, _| true));
    cache.drop_parked_off_theme(0);
    assert!(parked_for(cache.parked(), IDENT, size(r)).is_some(), "the same theme stays");
    cache.drop_parked_off_theme(1);
    assert_eq!(parked_for(cache.parked(), IDENT, size(r)), None);
}

/// The panel re-opened (a revive, or a parked store taken over by a new enter) after a re-emit
/// that only renumbered its nodes paints nothing.
#[test]
fn a_reopen_of_the_same_subtree_paints_nothing() {
    let mut rng = Rng(17);
    let nodes = panel(&mut rng);
    let (doc, span) = build(&[outside(&mut rng)], &nodes);
    let r = layer_rect(&doc, span).expect("paints");
    let mut cache = Cache::new();
    cache.now_mut().take((&doc.boxes, &doc.texts), span, (r, (0, 260)), Inputs::new(0, None));
    cache.hold_now(0);
    // Renumbered: three more nodes came before it.
    let prefix = vec![outside(&mut rng), outside(&mut rng), outside(&mut rng), outside(&mut rng)];
    let (doc2, span2) = build(&prefix, &nodes);
    assert_ne!(span, span2);
    let lists = (doc2.boxes.as_slice(), doc2.texts.as_slice());
    cache.now_mut().take(lists, span2, (r, (0, 260)), Inputs::new(0, None));
    assert_eq!(cache.decide(0), Reuse::Keep, "a revive keeps its store");
    // Dropped, parked, taken over by a new enter in another slot at the same place.
    assert!(cache.park(0, (IDENT, size(r), (0, 260)), PARKED_KEY, |_, _| true));
    assert!(cache.wants(0), "the slot gave its snapshot to the parked store");
    let j = parked_for(cache.parked(), IDENT, size(r)).expect("parked");
    assert_eq!(cache.unpark(j, 2), Some((0, 260)));
    cache.now_mut().take(lists, span2, (r, (0, 260)), Inputs::new(0, None));
    assert_eq!(cache.decide(2), Reuse::Keep, "a re-enter of the same subtree paints nothing");
}

/// When the store has no room, the parked stores go before a ghost — and a ghost keeps its place.
#[test]
fn a_parked_store_gives_way_before_a_ghost() {
    use crate::store_alloc::Outcome;
    assert_eq!(give_way(Outcome::Full, (true, true, true)), GiveWay::Parked);
    assert_eq!(give_way(Outcome::Full, (false, true, true)), GiveWay::Ghosts("room"));
    assert_eq!(give_way(Outcome::Full, (false, false, true)), GiveWay::Motions);
    assert_eq!(give_way(Outcome::Full, (false, false, false)), GiveWay::Band);
    assert_eq!(give_way(Outcome::Compacted, (true, true, false)), GiveWay::Parked);
    assert_eq!(give_way(Outcome::Compacted, (false, true, false)), GiveWay::Ghosts("store"));
    assert_eq!(give_way(Outcome::Kept, (true, true, true)), GiveWay::Placed);
    // A ghost and a parked store fill the store; a new layer fits only when the parked store
    // gives way — the ghost keeps its place.
    let mut alloc = StoreAlloc::new(100, 0, 100);
    let ghost = Item { key: 2, w: 100, h: 40 };
    let parked = Item { key: PARKED_KEY | 1, w: 100, h: 40 };
    assert_eq!(alloc.plan(&[ghost, parked]), Outcome::Kept);
    let at = alloc.place_of(2);
    let new = Item { key: LAYER_KEY | 5, w: 100, h: 50 };
    let outcome = alloc.plan(&[ghost, parked, new]);
    assert_eq!(outcome, Outcome::Full);
    assert_eq!(give_way(outcome, (true, true, true)), GiveWay::Parked);
    assert_eq!(alloc.plan(&[ghost, new]), Outcome::Kept);
    assert_eq!(alloc.place_of(2), at, "the ghost did not move");
}

/// A layer holding a scroll band: the band's content is drawn in its band, never in the store,
/// so a change there (a scrolled row) keeps the store — the snapshot takes only what the layer's
/// pass paints (board 2026-11-21T07-37-47: 11 re-opens of band-holding layers repainted whole).
#[test]
fn a_change_inside_a_band_keeps_the_layer_s_store() {
    let mut rng = Rng(11);
    let (doc, span) = build(&[], &panel(&mut rng));
    let band_item = span.0 + 1;
    let mut scrolled = Doc { boxes: doc.boxes.clone(), texts: doc.texts.clone() };
    if let Some(b) = scrolled.boxes.iter_mut().find(|b| b.node_id == band_item) {
        b.rect.y += FxPx::new(40);
    }
    let r = layer_rect(&doc, span).expect("the panel paints");
    let painted = |b: &LayoutBox| b.node_id != band_item;
    let take = |d: &Doc| {
        let mut s = Snapshot::default();
        s.take_painted((&d.boxes, &d.texts), span, (r, (0, 0)), Inputs::new(0, None), &painted);
        s
    };
    assert_eq!(reuse(&take(&doc), &take(&scrolled)), Reuse::Keep);
    // Taken whole, the same change is the store's (the old rule): not kept.
    let (a, b) = (
        snap(&doc, span, (0, 0), Inputs::new(0, None)),
        snap(&scrolled, span, (0, 0), Inputs::new(0, None)),
    );
    assert_ne!(reuse(&a, &b), Reuse::Keep);
}

/// The line names why a store is painted whole: no snapshot yet, then another theme.
#[test]
fn a_whole_repaint_names_its_reason() {
    let mut rng = Rng(5);
    let (doc, span) = build(&[], &panel(&mut rng));
    let r = layer_rect(&doc, span).expect("paints");
    let mut cache = Cache::new();
    cache.now_mut().take((&doc.boxes, &doc.texts), span, (r, (0, 0)), Inputs::new(0, None));
    assert_eq!(cache.why_whole(0), "first");
    cache.hold_now(0);
    cache.now_mut().take((&doc.boxes, &doc.texts), span, (r, (0, 0)), Inputs::new(1, None));
    assert_eq!(cache.why_whole(0), "inputs");
}
