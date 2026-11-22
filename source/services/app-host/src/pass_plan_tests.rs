// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Band parity of a planned pass (TASK-0367 P1): a scene with everything a pass paints — a glass
//! panel with its shadow, capsule keys with border, shadow and shine, a stroked icon, an animated
//! box, a scroll layer's content at its view, text runs with a halo, an alignment, a clip and an
//! ellipsis, a focused field with its selection — painted whole, row after row, and then in 1 to
//! 8 bands on threads, in any order: every byte the same. The pattern of `nexus-svg`'s
//! `band_parity.rs`.

use super::*;
use nexus_layout_types::{
    BoxShadow, CornerRadius, EdgeBorder, FxPx, Material, PathPoint, PathShape, Rect, Rgba8,
    ShapeKind, TextAlign, TextShadow, TextStyle, VisualStyle,
};
use nexus_text_baked::measure_text::BakedTextMeasure;

pub(super) const W: i32 = 480;
pub(super) const H: i32 = 360;
/// The window's frost as the page base (premultiplied): translucent, so the straight-alpha
/// conversion is exercised on every pixel.
pub(super) const GROUND: [u8; 4] = [40, 36, 32, 200];

pub(super) fn rect((x, y, w, h): (i32, i32, i32, i32)) -> Rect {
    Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h))
}

pub(super) fn boxed(node_id: usize, at: (i32, i32, i32, i32), visual: VisualStyle) -> LayoutBox {
    LayoutBox { node_id, rect: rect(at), visual, ..LayoutBox::default() }
}

pub(super) fn shadow(y: i32, blur: i32, a: u8) -> Option<BoxShadow> {
    Some(BoxShadow {
        offset_x: FxPx::new(0),
        offset_y: FxPx::new(y),
        blur_radius: FxPx::new(blur),
        spread: FxPx::new(0),
        color: Rgba8::new(0, 0, 0, a),
    })
}

pub(super) fn run(node: usize, content: &str, align: TextAlign) -> TextRun {
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

/// The scene a pass paints, every box picked (node ids are pre-order, `boxes[id - 1]`).
pub(super) struct Scene {
    boxes: Vec<LayoutBox>,
    texts: Vec<TextRun>,
    picks: Vec<u32>,
    pick_anims: Vec<i16>,
    runs: Vec<(u32, u32)>,
    anims: Vec<NodeAnim>,
    views: Vec<ScrollView>,
}

pub(super) fn scene() -> Scene {
    let mut boxes = vec![
        // 1: the glass panel with its drop shadow.
        boxed(
            1,
            (24, 20, 300, 220),
            VisualStyle {
                background: Some(Rgba8::new(30, 30, 40, 160)),
                corner_radius: CornerRadius::uniform(FxPx::new(20)),
                material: Some(Material::Panel),
                shadow: shadow(12, 24, 90),
                ..VisualStyle::default()
            },
        ),
    ];
    // 2..=7: capsule keys, border, shadow and shine, two rows.
    for i in 0..6 {
        let (x, y) = (40 + (i % 3) * 90, 60 + (i / 3) * 70);
        boxes.push(boxed(
            2 + i as usize,
            (x, y, 80, 56),
            VisualStyle {
                background: Some(Rgba8::new(255, 255, 255, 40 + 20 * i as u8)),
                corner_radius: CornerRadius::uniform(FxPx::new(28)),
                border: EdgeBorder::all(FxPx::new(1), Rgba8::new(255, 255, 255, 60)),
                shadow: shadow(1, 3, 40),
                inset_highlight: Some(Rgba8::new(255, 255, 255, 50)),
                ..VisualStyle::default()
            },
        ));
    }
    // 8: a stroked icon.
    let paths = (0..4)
        .map(|p| PathShape {
            points: (0..8)
                .map(|i| PathPoint::new((i * 140) as u16, ((i * 53 + p * 210) % 1000) as u16))
                .collect(),
            closed: false,
        })
        .collect();
    boxes.push(boxed(
        8,
        (340, 30, 32, 32),
        VisualStyle {
            background: Some(Rgba8::new(230, 230, 240, 255)),
            shape: ShapeKind::Stroke { paths, width_milli: 125 },
            ..VisualStyle::default()
        },
    ));
    // 9: an animated card (faded, moved, scaled).
    boxes.push(boxed(
        9,
        (330, 90, 120, 80),
        VisualStyle {
            background: Some(Rgba8::new(90, 140, 250, 220)),
            corner_radius: CornerRadius::uniform(FxPx::new(12)),
            ..VisualStyle::default()
        },
    ));
    // 10: a scroll container; 11, 12: its content, placed by its view.
    boxes.push(boxed(
        10,
        (24, 260, 300, 80),
        VisualStyle { background: Some(Rgba8::new(20, 20, 24, 255)), ..VisualStyle::default() },
    ));
    for (i, y) in [(0usize, 270), (1, 330)] {
        let mut b = boxed(
            11 + i,
            (30, y, 280, 48),
            VisualStyle {
                background: Some(Rgba8::new(60, 60, 70, 255)),
                corner_radius: CornerRadius::uniform(FxPx::new(8)),
                ..VisualStyle::default()
            },
        );
        b.scroll_root = Some(10);
        boxes.push(b);
    }
    // 13: a label with a halo, centred; 14: a right-aligned label cut by its clip;
    // 15: a label too long for its box (an ellipsis); 16: the focused field.
    let mut halo = boxed(13, (340, 190, 120, 20), VisualStyle::default());
    halo.visual.text_shadows = [
        Some(TextShadow {
            offset_x: 0,
            offset_y: 1,
            blur_radius: 2,
            color: Rgba8::new(0, 0, 0, 90),
        }),
        None,
    ];
    boxes.push(halo);
    let mut clipped = boxed(14, (40, 200, 260, 20), VisualStyle::default());
    clipped.clip_rect = Some(rect((40, 204, 200, 12)));
    boxes.push(clipped);
    boxes.push(boxed(15, (340, 220, 70, 20), VisualStyle::default()));
    boxes.push(boxed(
        16,
        (340, 250, 130, 28),
        VisualStyle { background: Some(Rgba8::new(255, 255, 255, 30)), ..VisualStyle::default() },
    ));
    let texts = vec![
        run(13, "Halo label", TextAlign::Center),
        run(14, "A label its clip cuts at the top and bottom", TextAlign::Right),
        run(15, "Far too long for its box", TextAlign::Left),
        run(16, "selected text in a field", TextAlign::Left),
    ];
    let picks = (0..boxes.len() as u32).collect::<Vec<_>>();
    let anims =
        vec![NodeAnim { opacity: 160, dx: -6, dy: 9, scale_pct: 94, ..NodeAnim::identity(9) }];
    let pick_anims = boxes
        .iter()
        .map(|b| anims.iter().position(|a| a.node_id == b.node_id).map_or(-1, |i| i as i16))
        .collect();
    let runs = texts.iter().enumerate().map(|(ti, t)| ((t.node - 1) as u32, ti as u32)).collect();
    let views = vec![ScrollView { root: 10, clip: (24, 260, 324, 340), dx: 0, dy: 14 }];
    Scene { boxes, texts, picks, pick_anims, runs, anims, views }
}

impl Scene {
    pub(super) fn plan(&self) -> PassPlan<'_> {
        PassPlan {
            width: W,
            boxes: &self.boxes,
            picks: &self.picks,
            pick_anims: &self.pick_anims,
            runs: &self.runs,
            texts: &self.texts,
            anims: &self.anims,
            caret: Some(CaretPaint {
                nid: 16,
                selection: Some((8, 64)),
                highlight: [250, 140, 90, 96],
                scroll: 12,
            }),
            views: &self.views,
        }
    }
}

pub(super) const ROW_BYTES: usize = W as usize * 4;

/// The whole surface, row after row, as the probe paints its base.
pub(super) fn whole(plan: &PassPlan<'_>) -> Vec<u8> {
    let mut out = vec![0u8; ROW_BYTES * H as usize];
    for (y, row) in out.chunks_exact_mut(ROW_BYTES).enumerate() {
        plan.paint_ground_row(row, y as i32, GROUND);
    }
    out
}

/// `[0, H)` cut into `n` bands as even as the rows allow.
fn bands(n: usize) -> Vec<(i32, i32)> {
    (0..n).map(|i| ((H as usize * i / n) as i32, (H as usize * (i + 1) / n) as i32)).collect()
}

/// The plan is shared by reference across threads: it must be `Sync`.
#[test]
fn a_plan_is_shared_across_threads() {
    fn shared<T: Sync>(_: &T) {}
    let s = scene();
    shared(&s.plan());
}

/// The scene paints something on every kind of row (the parity below would hold trivially on
/// a blank surface).
#[test]
fn the_scene_paints_boxes_text_and_the_ground() {
    let s = scene();
    let pixels = whole(&s.plan());
    let ground_only = {
        let mut row = vec![0u8; ROW_BYTES];
        PassPlan { picks: &[], runs: &[], ..s.plan() }.paint_ground_row(&mut row, 0, GROUND);
        row
    };
    let painted_rows =
        pixels.chunks_exact(ROW_BYTES).filter(|r| *r != ground_only.as_slice()).count();
    assert!(painted_rows > H as usize * 3 / 4, "{painted_rows} of {H} rows carry the scene");
    // The text pass draws: the rows of the halo label differ from the same rows without runs.
    let no_text = whole(&PassPlan { runs: &[], ..s.plan() });
    let text_rows = (188..215)
        .filter(|&y| {
            let at = y * ROW_BYTES;
            pixels[at..at + ROW_BYTES] != no_text[at..at + ROW_BYTES]
        })
        .count();
    assert!(text_rows > 8, "the glyph pass painted {text_rows} rows");
}

/// Splits 1..8, each band painted on its own thread into its own buffer, then joined: equal to
/// the whole, byte for byte.
#[test]
fn bands_painted_on_threads_equal_the_whole_surface() {
    let s = scene();
    let plan = s.plan();
    let reference = whole(&plan);
    for n in 1..=8 {
        let mut joined = vec![0u8; reference.len()];
        std::thread::scope(|scope| {
            let mut rest = joined.as_mut_slice();
            for (y0, y1) in bands(n) {
                let (band, tail) = rest.split_at_mut((y1 - y0) as usize * ROW_BYTES);
                rest = tail;
                let plan = &plan;
                scope.spawn(move || plan.paint_ground_band(band, y0, GROUND));
            }
        });
        assert!(joined == reference, "{n} bands differ from the whole surface");
    }
}

/// Bands painted last to first, each into a buffer holding garbage: the same pixels — a row
/// owes nothing to the rows painted before it, nor to what its buffer held.
#[test]
fn bands_in_any_order_over_any_buffer_equal_the_whole_surface() {
    let s = scene();
    let plan = s.plan();
    let reference = whole(&plan);
    for n in [3, 5, 8] {
        let mut joined: Vec<u8> = (0..reference.len()).map(|i| (i * 131 + 7) as u8).collect();
        for (y0, y1) in bands(n).into_iter().rev() {
            let at = y0 as usize * ROW_BYTES;
            plan.paint_ground_band(&mut joined[at..y1 as usize * ROW_BYTES], y0, GROUND);
        }
        assert!(joined == reference, "{n} bands in reverse differ");
    }
}

/// Every row painted alone is its row of the whole surface.
#[test]
fn a_row_alone_equals_its_row_in_the_whole() {
    let s = scene();
    let plan = s.plan();
    let reference = whole(&plan);
    let mut row = vec![0u8; ROW_BYTES];
    for y in 0..H {
        plan.paint_ground_row(&mut row, y, GROUND);
        let at = y as usize * ROW_BYTES;
        assert!(row == reference[at..at + ROW_BYTES], "row {y} differs");
    }
}

/// A band buffer that is not a whole number of rows paints its whole rows and leaves the rest.
#[test]
fn test_reject_a_partial_row_in_a_band() {
    let s = scene();
    let plan = s.plan();
    let reference = whole(&plan);
    let mut band = vec![7u8; ROW_BYTES * 2 + 10];
    plan.paint_ground_band(&mut band, 100, GROUND);
    assert!(band[..ROW_BYTES * 2] == reference[100 * ROW_BYTES..102 * ROW_BYTES]);
    assert!(band[ROW_BYTES * 2..].iter().all(|&b| b == 7), "the partial row is left alone");
    // A plan of no width paints nothing.
    let mut untouched = vec![7u8; 64];
    PassPlan { width: 0, ..plan }.paint_ground_band(&mut untouched, 0, GROUND);
    assert!(untouched.iter().all(|&b| b == 7));
}

/// TASK-0367 P3: a pass planned over columns `[c0, c1)` — its boxes picked by
/// [`meets_cols`], its runs those of its picked boxes, as `prepare_picks_with` does — paints
/// there exactly what the whole row paints, over a row holding garbage, and leaves every other
/// pixel of the row as it was. Windows of 1 to 120 columns swept across the scene: through the
/// panel's corner and shadow, the keys, the stroked icon, the animated card, the scroll view,
/// the halo, the clipped and the cut label, the selection.
#[test]
fn columns_painted_alone_equal_the_whole_row_there() {
    let s = scene();
    let reference = whole(&s.plan());
    let garbage: Vec<u8> = (0..ROW_BYTES).map(|i| (i * 89 + 3) as u8).collect();
    let mut windows = 0;
    for c0 in (0..W).step_by(37).chain([W - 1]) {
        for width in [1, 7, 33, 120] {
            let cols = (c0, (c0 + width).min(W));
            let mut picks = Vec::new();
            let mut pick_anims = Vec::new();
            for (i, b) in s.boxes.iter().enumerate() {
                let dx = b.scroll_root.and_then(|r| s.views.iter().find(|v| v.root == r));
                let anim = s.anims.iter().position(|a| a.node_id == b.node_id);
                if meets_cols(b, dx.map_or(0, |v| v.dx), anim.is_some(), cols) {
                    picks.push(i as u32);
                    pick_anims.push(anim.map_or(-1, |a| a as i16));
                }
            }
            let runs: Vec<(u32, u32)> =
                s.runs.iter().copied().filter(|(bi, _)| picks.contains(bi)).collect();
            let plan = PassPlan { picks: &picks, pick_anims: &pick_anims, runs: &runs, ..s.plan() };
            let (b0, b1) = (cols.0 as usize * 4, cols.1 as usize * 4);
            for y in 0..H {
                let mut row = garbage.clone();
                plan.paint_ground_cols(&mut row, y, GROUND, cols);
                let at = y as usize * ROW_BYTES;
                assert!(
                    row[b0..b1] == reference[at + b0..at + b1],
                    "columns {cols:?} of row {y} differ from the whole row"
                );
                assert!(
                    row[..b0] == garbage[..b0] && row[b1..] == garbage[b1..],
                    "columns {cols:?} of row {y} wrote outside them"
                );
            }
            windows += 1;
        }
    }
    assert!(windows > 50);
}

/// Picking by columns leaves boxes out: a window right of everything picks nothing but what may
/// reach it, and the painted columns still equal the whole row (the ground alone).
#[test]
fn test_reject_a_box_whose_pixels_cannot_reach_the_columns() {
    let s = scene();
    let far = (476, 480);
    let picked: Vec<usize> =
        s.boxes.iter().filter(|b| meets_cols(b, 0, false, far)).map(|b| b.node_id).collect();
    assert!(!picked.contains(&1), "the panel ends at 324 + its shadow: {picked:?}");
    assert!(!picked.contains(&2), "the first key ends at 120");
    // The halo label (340..460, a 2 px blur) and the field (340..470) reach 476 with their
    // overhang only where they truly extend: the field's right edge is 470 + 2 < 476.
    assert!(!picked.contains(&16), "{picked:?}");
    // An animated box is picked wherever the window is: its transform moves it.
    assert!(meets_cols(&s.boxes[8], 0, true, (0, 1)));
    // An empty window picks nothing and paints nothing.
    let mut row = vec![9u8; ROW_BYTES];
    s.plan().paint_ground_cols(&mut row, 10, GROUND, (200, 200));
    assert!(row.iter().all(|&b| b == 9));
}
