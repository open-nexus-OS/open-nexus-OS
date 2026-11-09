// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The circular `ProgressBar` (design artifacts `ring.jsx`): a `fill` track circle and
//! the tinted arc from 12 o'clock clockwise to `value` with round caps — both stroked polylines
//! (`ShapeKind::Stroke`) in the ring's box, the stroke width and radius per size — the value
//! in the centre from md up, the label (and below md the value) beside it. Indeterminate is the
//! static 28 % arc; its turning is host motion. Split from lib.rs (the linear bar).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: lib tests (circular_*)
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, FlexItem, FontWeight, FxPx, Justify, LayoutNode, PathPoint, PathShape,
    Position, Rgba8, ShapeKind, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, ControlSize, TextStyleToken, Tokens};

use crate::{text, ProgressBar};

/// The indeterminate arc's share of the turn (percent).
pub(crate) const SPIN_ARC: usize = 28;

/// One size's ring: (diameter px, stroke in tenths of a px, centre text style).
pub(crate) fn ring_metrics(size: ControlSize) -> (i32, i32, Option<TextStyleToken>) {
    match size {
        ControlSize::Xs => (16, 25, None),
        ControlSize::Sm => (24, 30, None),
        ControlSize::Md => (36, 40, Some(TextStyleToken::Caption2)),
        ControlSize::Lg => (48, 50, Some(TextStyleToken::Footnote)),
        ControlSize::Xl => (64, 60, Some(TextStyleToken::Headline)),
    }
}

/// The circle through the stroke's centre line, from 12 o'clock clockwise over `percent` %
/// of the turn, in the `0..1000` viewbox of a `d`-px box stroked `sw10`/10 px wide.
pub(crate) fn arc(percent: usize, d: i32, sw10: i32) -> PathShape {
    // Radius of the centre line: (d − sw) / 2 of the box, in thousandths.
    let r = (d * 10 - sw10) * 500 / (d * 10);
    let points: Vec<PathPoint> = TURN[..=percent.min(100)]
        .iter()
        .map(|&(sin, cos)| {
            let x = 500 + r * i32::from(sin) / 10_000;
            let y = 500 - r * i32::from(cos) / 10_000;
            PathPoint::new(x.clamp(0, 1000) as u16, y.clamp(0, 1000) as u16)
        })
        .collect();
    PathShape { points, closed: false }
}

/// A stroked ring layer filling the ring box (absolute: the layers stack).
fn layer(path: PathShape, sw10: i32, d: i32, color: Rgba8) -> LayoutNode {
    let size = Some(FxPx::new(d));
    LayoutNode::Stack(
        Stack {
            min_width: size,
            max_width: size,
            min_height: size,
            max_height: size,
            item: FlexItem { position: Position::Absolute, ..FlexItem::default() },
            ..Stack::default()
        },
        VisualStyle {
            background: Some(color),
            shape: ShapeKind::Stroke {
                paths: alloc::vec![path],
                width_milli: (sw10 * 100 / d) as u16,
            },
            ..VisualStyle::default()
        },
        Vec::new(),
    )
}

/// The circular indicator: ring (track · arc · centre value) and the label column beside it.
pub(crate) fn build(bar: &ProgressBar, tokens: &dyn Tokens) -> LayoutNode {
    let (d, sw10, centre) = ring_metrics(bar.size);
    let tint = tokens.color(bar.color);
    let percent = if bar.indeterminate { SPIN_ARC } else { bar.value as usize };
    // The centre value (md up): the number at 600 in `label`, then "%" at 500, secondary.
    let mut ring_kids =
        alloc::vec![layer(arc(100, d, sw10), sw10, d, tokens.color(ColorToken::Fill))];
    if percent > 0 {
        ring_kids.push(layer(arc(percent, d, sw10), sw10, d, tint));
    }
    if let (true, false, Some(style)) = (bar.show_value, bar.indeterminate, centre) {
        let number = style.text_style(tokens.color(ColorToken::Label), Some(FontWeight::Semibold));
        let sign =
            style.text_style(tokens.color(ColorToken::LabelSecondary), Some(FontWeight::Medium));
        ring_kids.push(LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                gap: FxPx::new(1),
                align: Align::Center,
                justify: Justify::Center,
                min_width: Some(FxPx::new(d)),
                max_width: Some(FxPx::new(d)),
                min_height: Some(FxPx::new(d)),
                max_height: Some(FxPx::new(d)),
                ..Stack::default()
            },
            VisualStyle::default(),
            alloc::vec![text(alloc::format!("{}", bar.value), number), text("%", sign)],
        ));
    }
    let size = Some(FxPx::new(d));
    let ring = LayoutNode::Stack(
        Stack {
            min_width: size,
            max_width: size,
            min_height: size,
            max_height: size,
            item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
            ..Stack::default()
        },
        VisualStyle::default(),
        ring_kids,
    );
    let side_value = bar.show_value && !bar.indeterminate && centre.is_none();
    let mut row = alloc::vec![ring];
    if bar.label.is_some() || side_value {
        let mut col: Vec<LayoutNode> = Vec::new();
        if let Some(label) = &bar.label {
            let s = bar
                .label_style()
                .text_style(tokens.color(ColorToken::Label), Some(FontWeight::Medium));
            col.push(text(label.as_str(), s));
        }
        if side_value {
            let small = match bar.size {
                ControlSize::Xs => TextStyleToken::Caption2,
                _ => TextStyleToken::Caption,
            };
            let s = small.text_style(tokens.color(ColorToken::LabelSecondary), None);
            col.push(text(bar.value_text(), s));
        }
        row.push(LayoutNode::Stack(
            Stack { direction: Direction::Column, gap: FxPx::new(2), ..Stack::default() },
            VisualStyle::default(),
            col,
        ));
    }
    LayoutNode::Stack(
        Stack {
            id: bar.id,
            direction: Direction::Row,
            gap: FxPx::new(8),
            align: Align::Center,
            ..Stack::default()
        },
        VisualStyle::default(),
        row,
    )
}

/// `(sin, cos) × 10 000` at every percent of a turn, clockwise from 12 o'clock.
const TURN: [(i16, i16); 101] = [
    (0, 10000),
    (628, 9980),
    (1253, 9921),
    (1874, 9823),
    (2487, 9686),
    (3090, 9511),
    (3681, 9298),
    (4258, 9048),
    (4818, 8763),
    (5358, 8443),
    (5878, 8090),
    (6374, 7705),
    (6845, 7290),
    (7290, 6845),
    (7705, 6374),
    (8090, 5878),
    (8443, 5358),
    (8763, 4818),
    (9048, 4258),
    (9298, 3681),
    (9511, 3090),
    (9686, 2487),
    (9823, 1874),
    (9921, 1253),
    (9980, 628),
    (10000, 0),
    (9980, -628),
    (9921, -1253),
    (9823, -1874),
    (9686, -2487),
    (9511, -3090),
    (9298, -3681),
    (9048, -4258),
    (8763, -4818),
    (8443, -5358),
    (8090, -5878),
    (7705, -6374),
    (7290, -6845),
    (6845, -7290),
    (6374, -7705),
    (5878, -8090),
    (5358, -8443),
    (4818, -8763),
    (4258, -9048),
    (3681, -9298),
    (3090, -9511),
    (2487, -9686),
    (1874, -9823),
    (1253, -9921),
    (628, -9980),
    (0, -10000),
    (-628, -9980),
    (-1253, -9921),
    (-1874, -9823),
    (-2487, -9686),
    (-3090, -9511),
    (-3681, -9298),
    (-4258, -9048),
    (-4818, -8763),
    (-5358, -8443),
    (-5878, -8090),
    (-6374, -7705),
    (-6845, -7290),
    (-7290, -6845),
    (-7705, -6374),
    (-8090, -5878),
    (-8443, -5358),
    (-8763, -4818),
    (-9048, -4258),
    (-9298, -3681),
    (-9511, -3090),
    (-9686, -2487),
    (-9823, -1874),
    (-9921, -1253),
    (-9980, -628),
    (-10000, 0),
    (-9980, 628),
    (-9921, 1253),
    (-9823, 1874),
    (-9686, 2487),
    (-9511, 3090),
    (-9298, 3681),
    (-9048, 4258),
    (-8763, 4818),
    (-8443, 5358),
    (-8090, 5878),
    (-7705, 6374),
    (-7290, 6845),
    (-6845, 7290),
    (-6374, 7705),
    (-5878, 8090),
    (-5358, 8443),
    (-4818, 8763),
    (-4258, 9048),
    (-3681, 9298),
    (-3090, 9511),
    (-2487, 9686),
    (-1874, 9823),
    (-1253, 9921),
    (-628, 9980),
    (0, 10000),
];
