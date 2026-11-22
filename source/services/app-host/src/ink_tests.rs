// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The ink invariant end to end on the host (RFC-0121): a base painted by the real row painter
//! (`pass_plan`, the probe's base pass over a clear ground) holds every pixel with alpha > 0
//! inside the ink `ink_of` computed for it — on a shell-like desktop (bar pills with a clock and
//! its halo, desktop icons with haloed labels, a dock of glass with its shadow, a widget card, a
//! flat scroll view), a greeter-like one (a clock over the wallpaper, a glass card with an
//! avatar, a field and a button) and 200 seeded random scenes (shadows, borders, radii, glass,
//! stroked icons, halos, clips, ellipses, scroll views). The pieces are at most `MAX_INK`,
//! pairwise disjoint and inside the base. The pattern of TASK-0367 P3's 200-edit oracle.

use super::*;
use crate::pass_plan::PassPlan;
use nexus_layout_types::{
    BoxShadow, CornerRadius, EdgeBorder, FxPx, Material, PathPoint, PathShape, Rect, Rgba8,
    ShapeKind, TextAlign, TextShadow, TextStyle, VisualStyle,
};
use nexus_scene_raster::ScrollView;
use nexus_text_baked::measure_text::BakedTextMeasure;
use std::vec::Vec;

fn rect((x, y, w, h): (i32, i32, i32, i32)) -> Rect {
    Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h))
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

fn halo() -> [Option<TextShadow>; 2] {
    [
        Some(TextShadow {
            offset_x: 0,
            offset_y: 1,
            blur_radius: 3,
            color: Rgba8::new(0, 0, 0, 120),
        }),
        None,
    ]
}

fn run(node: usize, content: &str, align: TextAlign) -> TextRun {
    let style = TextStyle { text_align: align, ..TextStyle::default() };
    TextRun {
        node,
        content: content.into(),
        font: BakedTextMeasure::font(&style),
        color: [240, 240, 245, 255],
        weight: style.font_weight,
        tracking: BakedTextMeasure::tracking(&style),
        line: BakedTextMeasure::line_box(&style) as i32,
        align: style.text_align,
    }
}

/// A laid-out base: pre-order boxes (`boxes[id - 1]`), their runs, the scroll views.
#[derive(Default)]
struct Scene {
    boxes: Vec<LayoutBox>,
    texts: Vec<TextRun>,
    views: Vec<ScrollView>,
}

impl Scene {
    /// One more box (its id the next pre-order id), with its run when `text` is given.
    fn push(&mut self, at: (i32, i32, i32, i32), visual: VisualStyle, text: Option<&str>) -> usize {
        let node_id = self.boxes.len() + 1;
        self.boxes.push(LayoutBox { node_id, rect: rect(at), visual, ..LayoutBox::default() });
        if let Some(t) = text {
            self.texts.push(run(node_id, t, TextAlign::Center));
        }
        node_id
    }

    fn last(&mut self) -> &mut LayoutBox {
        let n = self.boxes.len();
        &mut self.boxes[n - 1]
    }

    fn placed(&self, b: &LayoutBox) -> Placed {
        match b.scroll_root.and_then(|r| self.views.iter().find(|v| v.root == r)) {
            Some(v) => Placed::InView(v.clip),
            None => Placed::Here,
        }
    }

    fn ink(&self, size: (u32, u32)) -> Ink {
        ink_of(&self.boxes, &self.texts, size, false, |b| self.placed(b))
    }

    /// The base the probe paints: every box and run over the clear ground, straight alpha.
    fn paint(&self, (w, h): (u32, u32)) -> Vec<u8> {
        let picks: Vec<u32> = (0..self.boxes.len() as u32).collect();
        let pick_anims = vec![-1i16; picks.len()];
        let runs: Vec<(u32, u32)> =
            self.texts.iter().enumerate().map(|(ti, t)| ((t.node - 1) as u32, ti as u32)).collect();
        let plan = PassPlan {
            width: w as i32,
            boxes: &self.boxes,
            picks: &picks,
            pick_anims: &pick_anims,
            runs: &runs,
            texts: &self.texts,
            anims: &[],
            caret: None,
            views: &self.views,
        };
        let row_bytes = w as usize * 4;
        let mut out = vec![0u8; row_bytes * h as usize];
        for (y, row) in out.chunks_exact_mut(row_bytes).enumerate() {
            plan.paint_ground_row(row, y as i32, [0, 0, 0, 0]);
        }
        out
    }
}

/// The pixels with alpha > 0 that no rectangle of `ink` holds (`None`: the whole base).
fn outside(px: &[u8], (w, _): (u32, u32), ink: Option<&[Rect16]>) -> Vec<(u32, u32)> {
    let Some(ink) = ink else { return Vec::new() };
    let mut out = Vec::new();
    for (i, p) in px.chunks_exact(4).enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let held = ink.iter().any(|r| {
            let (rx, ry, rw, rh) = r.wide();
            x >= rx && y >= ry && x < rx + rw && y < ry + rh
        });
        if p[3] > 0 && !held {
            out.push((x, y));
        }
    }
    out
}

/// The invariant on one scene: every painted pixel inside the ink; at most `MAX_INK` disjoint
/// rects inside the base. Returns (pixels painted, pixels declared).
fn holds_every_painted_pixel(s: &Scene, size: (u32, u32), what: &str) -> (u64, u64) {
    let ink = s.ink(size);
    let px = s.paint(size);
    let rects = ink.rects().expect("a clear base with no motion declares its ink");
    assert!(rects.len() <= MAX_INK, "{what}: {} rects", rects.len());
    for (i, r) in rects.iter().enumerate() {
        let (x, y, w, h) = r.wide();
        assert!(w > 0 && h > 0 && x + w <= size.0 && y + h <= size.1, "{what}: {r:?}");
        assert!(!rects[i + 1..].iter().any(|o| r.meets(o)), "{what}: {r:?} shares pixels");
    }
    let missed = outside(&px, size, Some(rects));
    assert!(
        missed.is_empty(),
        "{what}: {} painted px outside the ink, first {:?}",
        missed.len(),
        missed.first()
    );
    let painted = px.chunks_exact(4).filter(|p| p[3] > 0).count() as u64;
    (painted, rects.iter().map(|r| u64::from(r.area())).sum())
}

const SHELL: (u32, u32) = (960, 600);

/// A desktop: bar pills (one with the clock and its halo), desktop icons with haloed labels,
/// a glass dock with its shadow and stroked icons, a widget card, a flat scroll view's list.
fn shell() -> Scene {
    let mut s = Scene::default();
    let pill = |a| VisualStyle {
        background: Some(Rgba8::new(20, 20, 30, a)),
        corner_radius: CornerRadius::uniform(FxPx::new(14)),
        material: Some(Material::Thin),
        ..VisualStyle::default()
    };
    s.push((0, 0, 960, 36), VisualStyle::default(), None); // the bar: a container
    s.push((8, 4, 120, 28), pill(110), Some("Activities"));
    s.push((400, 4, 160, 28), pill(140), Some("Thu 20 Nov 14:05"));
    s.last().visual.text_shadows = halo();
    s.push((860, 4, 92, 28), pill(110), None);
    for i in 0..3 {
        let y = 70 + i * 110;
        s.push(
            (30, y, 64, 64),
            VisualStyle {
                background: Some(Rgba8::new(80, 120, 230, 255)),
                corner_radius: CornerRadius::uniform(FxPx::new(16)),
                shadow: shadow(2, 6, 70),
                ..VisualStyle::default()
            },
            None,
        );
        s.push(
            (10, y + 70, 104, 20),
            VisualStyle { text_shadows: halo(), ..VisualStyle::default() },
            Some("Settings and more"),
        );
    }
    s.push(
        (240, 520, 480, 68),
        VisualStyle {
            background: Some(Rgba8::new(255, 255, 255, 70)),
            corner_radius: CornerRadius::uniform(FxPx::new(22)),
            material: Some(Material::Dock),
            shadow: shadow(8, 24, 90),
            border: EdgeBorder::all(FxPx::new(1), Rgba8::new(255, 255, 255, 60)),
            ..VisualStyle::default()
        },
        None,
    );
    let paths: Vec<PathShape> = (0..3)
        .map(|p| PathShape {
            points: (0..6)
                .map(|i| PathPoint::new((i * 190) as u16, ((i * 77 + p * 300) % 1000) as u16))
                .collect(),
            closed: false,
        })
        .collect();
    for i in 0..5 {
        s.push(
            (262 + i * 90, 532, 44, 44),
            VisualStyle {
                background: Some(Rgba8::new(240, 240, 250, 255)),
                shape: ShapeKind::Stroke { paths: paths.clone(), width_milli: 110 },
                ..VisualStyle::default()
            },
            None,
        );
    }
    s.push(
        (700, 80, 220, 140),
        VisualStyle {
            background: Some(Rgba8::new(30, 30, 40, 150)),
            corner_radius: CornerRadius::uniform(FxPx::new(18)),
            material: Some(Material::Card),
            shadow: shadow(10, 20, 80),
            inset_highlight: Some(Rgba8::new(255, 255, 255, 40)),
            ..VisualStyle::default()
        },
        Some("Weather 12°"),
    );
    // A flat scroll view: its content placed by the view, inside the viewport.
    let root = s.push((700, 260, 220, 200), VisualStyle::default(), None);
    for i in 0..6 {
        s.push(
            (706, 262 + i * 44, 208, 40),
            VisualStyle {
                background: Some(Rgba8::new(60, 60, 70, 200)),
                corner_radius: CornerRadius::uniform(FxPx::new(8)),
                ..VisualStyle::default()
            },
            Some("A notification row"),
        );
        s.last().scroll_root = Some(root);
    }
    s.views.push(ScrollView { root, clip: (700, 260, 920, 460), dx: 0, dy: 30 });
    s
}

const GREETER: (u32, u32) = (800, 560);

/// The greeter: a big clock with its halo over the wallpaper, a glass card with an avatar, the
/// user's name, a password field (border) and a button with its shadow.
fn greeter() -> Scene {
    let mut s = Scene::default();
    s.push(
        (250, 40, 300, 60),
        VisualStyle { text_shadows: halo(), ..VisualStyle::default() },
        Some("14:05"),
    );
    s.push(
        (240, 150, 320, 340),
        VisualStyle {
            background: Some(Rgba8::new(20, 20, 30, 120)),
            corner_radius: CornerRadius::uniform(FxPx::new(28)),
            material: Some(Material::Panel),
            shadow: shadow(16, 32, 100),
            ..VisualStyle::default()
        },
        None,
    );
    s.push(
        (360, 180, 80, 80),
        VisualStyle {
            background: Some(Rgba8::new(200, 160, 120, 255)),
            shape: ShapeKind::Circle,
            ..VisualStyle::default()
        },
        None,
    );
    s.push((260, 276, 280, 24), VisualStyle::default(), Some("Jenning"));
    s.push(
        (270, 330, 260, 40),
        VisualStyle {
            background: Some(Rgba8::new(255, 255, 255, 30)),
            corner_radius: CornerRadius::uniform(FxPx::new(10)),
            border: EdgeBorder::all(FxPx::new(1), Rgba8::new(255, 255, 255, 90)),
            ..VisualStyle::default()
        },
        Some("Password"),
    );
    s.push(
        (330, 400, 140, 44),
        VisualStyle {
            background: Some(Rgba8::new(60, 120, 250, 255)),
            corner_radius: CornerRadius::uniform(FxPx::new(22)),
            shadow: shadow(4, 10, 90),
            ..VisualStyle::default()
        },
        Some("Log in"),
    );
    s
}

#[test]
fn the_ink_holds_every_painted_pixel() {
    let (painted, declared) = holds_every_painted_pixel(&shell(), SHELL, "shell");
    let whole = u64::from(SHELL.0 * SHELL.1);
    assert!(painted > 0 && declared < whole / 2, "the shell's ink {declared} of {whole} px");
    let (painted, declared) = holds_every_painted_pixel(&greeter(), GREETER, "greeter");
    let whole = u64::from(GREETER.0 * GREETER.1);
    assert!(painted > 0 && declared < whole / 2, "the greeter's ink {declared} of {whole} px");
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn within(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo) as u64) as i32
    }

    fn one_in(&mut self, n: u64) -> bool {
        self.next() % n == 0
    }
}

const RANDOM: (u32, u32) = (320, 240);

/// A random clear base: boxes of every look, runs with halos, clips, a scroll view.
fn random(rng: &mut Rng) -> Scene {
    let mut s = Scene::default();
    let root = if rng.one_in(3) {
        let r = s.push((40, 60, 200, 120), VisualStyle::default(), None);
        s.views.push(ScrollView {
            root: r,
            clip: (40, 60, 240, 180),
            dx: rng.within(-20, 20),
            dy: rng.within(-60, 60),
        });
        Some(r)
    } else {
        None
    };
    for _ in 0..rng.within(1, 24) {
        let at =
            (rng.within(-30, 300), rng.within(-30, 220), rng.within(1, 140), rng.within(1, 90));
        let a = rng.within(0, 256) as u8;
        let visual = VisualStyle {
            background: (!rng.one_in(4)).then(|| Rgba8::new(200, 90, 40, a)),
            corner_radius: CornerRadius::uniform(FxPx::new(rng.within(0, 30))),
            border: if rng.one_in(3) {
                EdgeBorder::all(FxPx::new(2), Rgba8::new(255, 255, 255, 120))
            } else {
                EdgeBorder::default()
            },
            shadow: rng
                .one_in(3)
                .then(|| shadow(rng.within(-8, 12), rng.within(0, 30), 120))
                .flatten(),
            material: rng.one_in(5).then_some(Material::Panel),
            shape: if rng.one_in(6) { ShapeKind::Circle } else { ShapeKind::Rect },
            text_shadows: if rng.one_in(3) { halo() } else { [None, None] },
            ..VisualStyle::default()
        };
        let text = rng.one_in(2).then_some("Label that may be cut short");
        s.push(at, visual, text);
        if rng.one_in(5) {
            let (x, y) = (rng.within(0, 300), rng.within(0, 220));
            s.last().clip_rect = Some(rect((x, y, rng.within(1, 120), rng.within(1, 80))));
        }
        if let Some(r) = root.filter(|_| rng.one_in(3)) {
            s.last().scroll_root = Some(r);
        }
    }
    s
}

#[test]
fn the_ink_holds_every_painted_pixel_of_random_scenes() {
    let mut rng = Rng(0x0121_ba5e_1u64);
    for seed in 0..200 {
        let s = random(&mut rng);
        holds_every_painted_pixel(&s, RANDOM, &std::format!("random scene {seed}"));
    }
}

/// Merged down to the bound, the ink still holds every painted pixel — and the oracle sees a
/// merge that cuts one rectangle off instead of growing it (truncating the set to the bound).
#[test]
fn test_reject_a_merge_that_drops_a_pixel() {
    let mut s = Scene::default();
    // Thirty small disjoint marks spread over the base: far more than the bound.
    for i in 0..30 {
        let (x, y) = (12 + (i % 6) * 50, 12 + (i / 6) * 45);
        s.push(
            (x, y, 10, 10),
            VisualStyle {
                background: Some(Rgba8::new(255, 255, 255, 200)),
                ..VisualStyle::default()
            },
            None,
        );
    }
    let ink = s.ink(RANDOM);
    let rects = ink.rects().expect("declared");
    assert_eq!(rects.len(), MAX_INK, "merged down to the bound");
    let px = s.paint(RANDOM);
    assert!(outside(&px, RANDOM, Some(rects)).is_empty(), "the merge grew, it dropped nothing");
    // A merge that drops: every mark held by the first eight rects alone.
    let mut dropping = Set::new();
    for b in &s.boxes {
        let r = &b.rect;
        let e = (r.x.0 - 2, r.y.0 - 2, r.x.0 + r.width.0 + 2, r.y.0 + r.height.0 + 2);
        if dropping.n < MAX_INK {
            dropping.absorb(e);
        }
    }
    let cut_off = dropping.ink();
    assert!(!outside(&px, RANDOM, cut_off.rects()).is_empty(), "the oracle sees the dropped marks");
    // A set past the bound is never cut down by the writer: the whole base instead.
    let mut over = Set::new();
    for i in 0..(MAX_INK as i32 + 1) {
        over.absorb((i * 20, 0, i * 20 + 10, 10));
    }
    assert_eq!(over.ink(), Ink::WHOLE);
}

#[test]
fn a_moving_base_box_or_an_empty_base_declare_what_holds() {
    let s = shell();
    assert_eq!(ink_of(&s.boxes, &s.texts, SHELL, true, |b| s.placed(b)), Ink::WHOLE, "moving");
    assert_eq!(Ink::WHOLE.rects(), None);
    let empty = Scene::default().ink(SHELL);
    assert_eq!(empty.rects(), Some(&[][..]), "nothing painted: no body");
    // A box of another pass (a band's content, a motion layer's) is no ink of the base.
    let elsewhere = ink_of(&s.boxes, &s.texts, SHELL, false, |_| Placed::Elsewhere);
    assert_eq!(elsewhere.rects(), Some(&[][..]));
    // The structure carries it, and tells it was sent.
    let mut tree = SurfaceTree::EMPTY;
    let ink = s.ink(SHELL);
    assert!(!ink.sent_in(&tree));
    ink.put(&mut tree);
    assert!(ink.sent_in(&tree) && tree.ink() == ink.rects());
    // The same scene, the same ink, in the same order (stable layer keys).
    assert_eq!(shell().ink(SHELL), ink);
}
