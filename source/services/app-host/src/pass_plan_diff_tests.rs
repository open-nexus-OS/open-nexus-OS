// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The damage invariant, end to end on the host (TASK-0354 P8): a layout is edited, the diff
//! (`layout_diff::changed_rows`) names its rectangles, and painting those rectangles — by the
//! column pass the probe runs (`paint_ground_cols` over boxes picked by `meets_cols`) — into the
//! frame of the layout before gives the frame of the layout after, byte for byte. 200 seeded
//! edits, each on the layout the last one left: boxes come, go, move, grow, swap places, change
//! their look, their fade, their clip, their shadow, their glass; text runs change, come and
//! go; a scroll container's content moves. What the diff does not see is not edited here: an
//! animated node's transform and a scroll view's offset are named by the probe's plumbing.

use super::tests::{boxed, rect, run, shadow, whole, GROUND, H, ROW_BYTES, W};
use super::*;
use crate::band_math::Change;
use crate::layout_diff::{changed_rows, hides};
use nexus_layout_types::{
    CornerRadius, EdgeBorder, FxPx, Material, PathPoint, PathShape, Rgba8, ShapeKind, TextAlign,
    TextShadow, VisualStyle,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Plain,
    /// The scroll container: its content is placed by its view, inside its box.
    ScrollRoot,
    ScrollChild,
}

#[derive(Clone)]
struct Item {
    b: LayoutBox,
    text: Option<TextRun>,
    role: Role,
}

/// A layout as the probe retains it: boxes in pre-order, their runs, the base's scroll views.
struct Doc {
    boxes: Vec<LayoutBox>,
    texts: Vec<TextRun>,
    views: Vec<ScrollView>,
}

/// Numbers the items in pre-order, as a layout pass does: node ids, the runs' nodes, the
/// scroll content's root — and the root's view at identity (a flattened container's).
fn build(items: &[Item]) -> Doc {
    let root = items.iter().position(|i| i.role == Role::ScrollRoot);
    let mut doc = Doc { boxes: Vec::new(), texts: Vec::new(), views: Vec::new() };
    for (i, item) in items.iter().enumerate() {
        let mut b = item.b.clone();
        b.node_id = i + 1;
        b.scroll_root = root.filter(|_| item.role == Role::ScrollChild).map(|r| r + 1);
        if let Some(t) = &item.text {
            doc.texts.push(TextRun { node: i + 1, ..t.clone() });
        }
        doc.boxes.push(b);
    }
    if let Some(r) = root {
        let c = &items[r].b.rect;
        let clip = (c.x.0, c.y.0, c.x.0 + c.width.0, c.y.0 + c.height.0);
        doc.views.push(ScrollView { root: r + 1, clip, dx: 0, dy: 0 });
    }
    doc
}

impl Doc {
    /// The base pass over columns `cols` (`None`: every column), picked as
    /// `probe/paint.rs::prepare_picks_with` picks it: every box that may write there, the runs
    /// of the picked boxes that no later glass root hides.
    fn picks(&self, cols: Option<(i32, i32)>) -> (Vec<u32>, Vec<i16>, Vec<(u32, u32)>) {
        let mut picks = Vec::new();
        let mut runs = Vec::new();
        for (bi, b) in self.boxes.iter().enumerate() {
            if b.rect.width.0 <= 0 || b.rect.height.0 <= 0 {
                continue;
            }
            let ti = self.texts.binary_search_by_key(&b.node_id, |t| t.node).ok();
            let by_run = |c| ti.is_some_and(|ti| run_meets_cols(b, &self.texts[ti], 0, c));
            if cols.is_some_and(|c| !meets_cols(b, 0, false, c) && !by_run(c)) {
                continue;
            }
            picks.push(bi as u32);
            let hidden = self.boxes.iter().any(|g| hides(g, b));
            if let (false, Some(ti)) = (hidden, ti) {
                runs.push((bi as u32, ti as u32));
            }
        }
        let none = vec![-1; picks.len()];
        (picks, none, runs)
    }

    fn plan<'a>(&'a self, parts: &'a (Vec<u32>, Vec<i16>, Vec<(u32, u32)>)) -> PassPlan<'a> {
        PassPlan {
            width: W,
            boxes: &self.boxes,
            picks: &parts.0,
            pick_anims: &parts.1,
            runs: &parts.2,
            texts: &self.texts,
            anims: &[],
            caret: None,
            views: &self.views,
        }
    }

    /// The whole frame, row after row.
    fn frame(&self) -> Vec<u8> {
        let parts = self.picks(None);
        whole(&self.plan(&parts))
    }

    /// Rectangle `(x, y, w, h)` of this layout painted into `frame`, as `band.rs::redraw` paints
    /// a damage rectangle: its rows, its columns alone.
    fn repaint(&self, frame: &mut [u8], (x, y, w, h): (u32, u32, u32, u32)) {
        let cols = (x as i32, ((x + w) as i32).min(W));
        let parts = self.picks(Some(cols));
        let plan = self.plan(&parts);
        for row in y..(y + h).min(H as u32) {
            let at = row as usize * ROW_BYTES;
            plan.paint_ground_cols(&mut frame[at..at + ROW_BYTES], row as i32, GROUND, cols);
        }
    }
}

/// A small deterministic generator (xorshift): the same edits on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
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

/// The looks a box takes: flat, translucent, a card with its shadow, a capsule with border and
/// shine, a glass panel, a stroked icon, a container that paints nothing.
fn look(rng: &mut Rng) -> VisualStyle {
    let bg = Some(rng.colour());
    match rng.below(7) {
        0 => {
            VisualStyle { background: Some(Rgba8::new(40, 60, 90, 255)), ..VisualStyle::default() }
        }
        1 => VisualStyle { background: bg, ..VisualStyle::default() },
        2 => VisualStyle {
            background: bg,
            corner_radius: CornerRadius::uniform(FxPx::new(10)),
            shadow: shadow(4, 9, 80),
            ..VisualStyle::default()
        },
        3 => VisualStyle {
            background: bg,
            corner_radius: CornerRadius::uniform(FxPx::new(14)),
            border: EdgeBorder::all(FxPx::new(1), Rgba8::new(255, 255, 255, 70)),
            inset_highlight: Some(Rgba8::new(255, 255, 255, 50)),
            ..VisualStyle::default()
        },
        4 => VisualStyle {
            background: Some(Rgba8::new(30, 30, 40, 150)),
            corner_radius: CornerRadius::uniform(FxPx::new(16)),
            material: Some(Material::Panel),
            ..VisualStyle::default()
        },
        5 => VisualStyle {
            background: Some(Rgba8::new(230, 230, 240, 255)),
            shape: ShapeKind::Stroke {
                paths: vec![PathShape {
                    points: (0..6).map(|i| PathPoint::new(i * 200, (i * 371) % 1000)).collect(),
                    closed: false,
                }],
                width_milli: 125,
            },
            ..VisualStyle::default()
        },
        _ => VisualStyle::default(),
    }
}

const WORDS: [&str; 6] =
    ["Wi-Fi", "Bluetooth is on", "12:41", "A label too long for the box it is in", "Aus", "An"];

fn label(rng: &mut Rng) -> TextRun {
    let align = [TextAlign::Left, TextAlign::Center, TextAlign::Right][rng.below(3)];
    run(0, WORDS[rng.below(WORDS.len())], align)
}

fn halo() -> [Option<TextShadow>; 2] {
    [
        Some(TextShadow {
            offset_x: 0,
            offset_y: 1,
            blur_radius: 2,
            color: Rgba8::new(0, 0, 0, 90),
        }),
        None,
    ]
}

fn fresh(rng: &mut Rng) -> Item {
    let at =
        (rng.within(-10, W - 60), rng.within(-6, H - 30), rng.within(8, 150), rng.within(8, 90));
    let text = (rng.below(3) == 0).then(|| label(rng));
    Item { b: boxed(0, at, look(rng)), text, role: Role::Plain }
}

/// Where the edits start: a page ground, a bar with a button and its label, cards, a glass
/// panel over a label, a scroll container with content, a label with a halo.
fn start(rng: &mut Rng) -> Vec<Item> {
    let plain = |at, v: VisualStyle| Item { b: boxed(0, at, v), text: None, role: Role::Plain };
    let flat = |c: Rgba8| VisualStyle { background: Some(c), ..VisualStyle::default() };
    let mut items = vec![
        plain((0, 0, W, 28), flat(Rgba8::new(20, 20, 26, 230))),
        plain((300, 2, 64, 24), flat(Rgba8::new(60, 60, 70, 255))),
        Item {
            text: Some(run(0, "Panel", TextAlign::Center)),
            ..plain((304, 4, 56, 20), VisualStyle::default())
        },
        Item {
            role: Role::ScrollRoot,
            ..plain((20, 200, 220, 140), flat(Rgba8::new(24, 24, 30, 255)))
        },
    ];
    for k in 0..3 {
        let mut row = plain((28, 208 + k * 46, 200, 40), flat(Rgba8::new(60, 60, 70, 255)));
        row.role = Role::ScrollChild;
        row.text = Some(run(0, WORDS[k as usize], TextAlign::Left));
        items.push(row);
    }
    let mut halo_label = plain((260, 60, 150, 20), VisualStyle::default());
    halo_label.b.visual.text_shadows = halo();
    halo_label.text = Some(run(0, "Halo label", TextAlign::Left));
    items.push(halo_label);
    for _ in 0..6 {
        items.push(fresh(rng));
    }
    items
}

/// One edit of the layout; returns what it did (for a failing case's message).
fn edit(items: &mut Vec<Item>, rng: &mut Rng) -> &'static str {
    let k = rng.below(items.len());
    let root = items[k].role == Role::ScrollRoot;
    let op = rng.below(15);
    // The scroll container keeps its place and its box: its view is the plumbing's.
    if root && !matches!(op, 0 | 4) {
        items[k].b.visual.background = Some(rng.colour());
        return "recolour the scroll container";
    }
    let (many, swaps) =
        (items.len() > 8, k + 1 < items.len() && items[k + 1].role != Role::ScrollRoot);
    let b = &mut items[k].b;
    match op {
        0 => {
            b.visual.background = Some(rng.colour());
            "recolour"
        }
        1 => {
            let (dx, dy) = (rng.within(-40, 40), rng.within(-30, 30));
            b.rect = rect((b.rect.x.0 + dx, b.rect.y.0 + dy, b.rect.width.0, b.rect.height.0));
            "move"
        }
        2 => {
            let (dw, dh) = (rng.within(-20, 30), rng.within(-20, 30));
            let (w, h) = ((b.rect.width.0 + dw).max(4), (b.rect.height.0 + dh).max(4));
            b.rect = rect((b.rect.x.0, b.rect.y.0, w, h));
            "resize"
        }
        3 => {
            let h = (b.rect.height.0 + rng.within(1, 24)).max(4);
            b.rect = rect((b.rect.x.0, b.rect.y.0, b.rect.width.0, h));
            "grow down"
        }
        4 => {
            let at = rng.below(items.len() + 1);
            let item = fresh(rng);
            items.insert(at, item);
            "insert"
        }
        5 if many => {
            items.remove(k);
            "remove"
        }
        6 => {
            items[k].text = match rng.below(3) {
                0 => None,
                _ => Some(label(rng)),
            };
            "text"
        }
        7 => {
            if let Some(t) = items[k].text.as_mut() {
                t.color = [rng.next() as u8, 200, 220, 255];
            }
            "text colour"
        }
        8 => {
            b.opacity = [255, 128, 64][rng.below(3)];
            "fade"
        }
        9 => {
            let r = &b.rect;
            let inset = rect((r.x.0 + 3, r.y.0 + 2, r.width.0 - 8, r.height.0 - 5));
            b.clip_rect = if b.clip_rect.is_some() { None } else { Some(inset) };
            "clip"
        }
        10 => {
            b.visual.shadow = if b.visual.shadow.is_some() { None } else { shadow(3, 8, 90) };
            "shadow"
        }
        11 => {
            b.visual.text_shadows =
                if b.visual.text_shadows[0].is_some() { [None; 2] } else { halo() };
            "halo"
        }
        12 if swaps => {
            items.swap(k, k + 1);
            "swap"
        }
        13 => {
            b.visual = look(rng);
            "restyle"
        }
        _ => {
            let glass = b.visual.material.is_some();
            b.visual.material = if glass { None } else { Some(Material::Panel) };
            "glass"
        }
    }
}

/// 200 edits: the diff's rectangles painted into the frame before the edit are the frame after
/// it. A rectangle too few (a shadow's reach, a faded child, a run a glass root hides) leaves
/// old pixels standing and fails here with the edit that did it.
#[test]
fn a_diffs_rectangles_repaint_to_the_full_frame() {
    let surface = W as u64 * H as u64;
    let (named_px, small, quiet) = edits(0x9E37_79B9_7F4A_7C15, 200);
    // The bound is a bound: most edits name a small part of the surface, and the frames are
    // not equal by naming everything.
    assert!(small >= 120, "{small} of 200 edits named under a tenth of the surface");
    assert!(named_px < surface * 200 / 4, "{named_px} px named over 200 edits");
    assert!(quiet < 100, "{quiet} edits changed nothing: the edits are too tame");
}

/// `count` edits from `seed`, each checked: the pixels named in all, the edits that named
/// under a tenth of the surface, and those that named nothing.
fn edits(seed: u64, count: usize) -> (u64, usize, usize) {
    let mut rng = Rng(seed);
    let mut items = start(&mut rng);
    let mut doc = build(&items);
    let mut frame = doc.frame();
    let none = |_: usize| false;
    let surface = W as u64 * H as u64;
    let (mut named_px, mut small, mut quiet) = (0u64, 0, 0);
    for n in 0..count {
        let did = edit(&mut items, &mut rng);
        let next = build(&items);
        let texts = (doc.texts.as_slice(), next.texts.as_slice());
        let mut changed = changed_rows(&doc.boxes, &next.boxes, texts, (W, H), (&none, &none));
        let rects = match changed.base.take() {
            None => crate::band_math::PresentDamage::default(),
            Some(Change::Rects(r)) => r,
            Some(Change::Whole) => panic!("edit {n} ({did}): no bound for a layout in shape"),
        };
        for r in rects.rects() {
            next.repaint(&mut frame, *r);
        }
        let full = next.frame();
        if let Some(at) = frame.iter().zip(&full).position(|(a, b)| a != b) {
            let (x, y) = ((at % ROW_BYTES) / 4, at / ROW_BYTES);
            panic!(
                "edit {n} ({did}): pixel ({x},{y}) kept its old value; named {:?}",
                rects.rects()
            );
        }
        named_px += rects.px().min(surface);
        small += usize::from(rects.px() * 10 < surface);
        quiet += usize::from(rects.is_empty());
        doc = next;
    }
    (named_px, small, quiet)
}

/// A second seed, so a rule fitted to the first one's edits is not the proof.
#[test]
fn a_second_run_of_edits_repaints_to_the_full_frame_too() {
    let (_, small, _) = edits(0x2545_F491_4F6C_DD1D, 200);
    assert!(small >= 100, "{small} of 200 edits named under a tenth of the surface");
}

/// The same check catches a rectangle too few: without the margin's columns a moved shadow's
/// reach, here cut off by hand, leaves old pixels in the frame.
#[test]
fn test_reject_a_repaint_that_names_too_little() {
    let mut rng = Rng(7);
    let mut items = start(&mut rng);
    let doc = build(&items);
    items[1].b.visual.shadow = shadow(4, 12, 160);
    let next = build(&items);
    let mut frame = doc.frame();
    // The button's box alone, without its shadow's reach.
    next.repaint(&mut frame, (300, 2, 64, 24));
    assert!(frame != next.frame(), "the shadow's reach was not painted: the frames differ");
    // What the diff names is enough.
    let none = |_: usize| false;
    let texts = (doc.texts.as_slice(), next.texts.as_slice());
    let mut changed = changed_rows(&doc.boxes, &next.boxes, texts, (W, H), (&none, &none));
    let Some(Change::Rects(rects)) = changed.base.take() else { panic!("rectangles") };
    let mut frame = doc.frame();
    rects.rects().iter().for_each(|r| next.repaint(&mut frame, *r));
    assert!(frame == next.frame());
}
