// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The pieces of a list row (design `ListItem`): the count badge, the selection circle
//! and check, the disclosure chevron and the reorder handle — each sized and coloured from the
//! design artifacts, split out of `lib.rs` so the row builder reads as layout.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: src/tests.rs
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

use alloc::string::String;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeBorder, EdgeInsets, FlexItem, FontWeight, FxPx, Justify,
    LayoutNode, Rgba8, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, RadiusToken, TextStyleToken, Tokens};
use nexus_widget_icon::{Icon, LucideSymbol};
use nexus_widget_text::Text;

/// The design's check (`M4 12.5l5 5L20 6.5`) in the normalized `0..1000` viewbox.
const CHECK: &[&[(u16, u16)]] = &[&[(167, 521), (375, 729), (833, 271)]];
/// The reorder handle: three lines (`M4 8h16M4 12h16M4 16h16`).
const HANDLE: &[&[(u16, u16)]] =
    &[&[(167, 333), (833, 333)], &[(167, 500), (833, 500)], &[(167, 667), (833, 667)]];

/// The selection circle's diameter.
pub(crate) const CIRCLE: i32 = 22;

/// `color` at `pct` % of its own alpha.
pub(crate) fn fade(color: Rgba8, pct: u32) -> Rgba8 {
    Rgba8 { a: (u32::from(color.a) * pct / 100) as u8, ..color }
}

/// The ink that reads on a solid `tint` (the design's `onTint`).
pub(crate) fn on_tint(tint: ColorToken) -> ColorToken {
    match tint {
        ColorToken::Primary | ColorToken::Label => ColorToken::OnPrimary,
        _ => ColorToken::OnAccent,
    }
}

/// A fixed `w × h` box.
pub(crate) fn fixed(
    w: i32,
    h: i32,
    visual: VisualStyle,
    children: alloc::vec::Vec<LayoutNode>,
) -> LayoutNode {
    LayoutNode::Stack(
        Stack {
            direction: Direction::Row,
            align: Align::Center,
            justify: Justify::Center,
            min_width: Some(FxPx::new(w)),
            max_width: Some(FxPx::new(w)),
            min_height: Some(FxPx::new(h)),
            max_height: Some(FxPx::new(h)),
            ..Stack::default()
        },
        visual,
        children,
    )
}

/// The count badge: a 20 px capsule (min 20 wide, 0 8 padding), the tint with its on-ink in
/// footnote semibold.
pub(crate) fn badge(tokens: &dyn Tokens, text: String, tint: ColorToken) -> LayoutNode {
    let label = Text::new(text)
        .text_style(TextStyleToken::Footnote)
        .weight(FontWeight::Semibold)
        .color(on_tint(tint))
        .build(tokens);
    LayoutNode::Stack(
        Stack {
            direction: Direction::Row,
            padding: EdgeInsets::symmetric(FxPx::ZERO, FxPx::new(8)),
            align: Align::Center,
            justify: Justify::Center,
            min_width: Some(FxPx::new(20)),
            min_height: Some(FxPx::new(20)),
            max_height: Some(FxPx::new(20)),
            item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
            ..Stack::default()
        },
        VisualStyle {
            background: Some(tokens.color(tint)),
            corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
            ..VisualStyle::default()
        },
        alloc::vec![label],
    )
}

/// The leading selection circle: a 1.5 px `labelTertiary` ring at rest (drawn 2 px wide at 75 %
/// of its alpha — the same ink on the integer pixel grid); selected, an accent disc with a 13 px
/// `onAccent` check (stroke 3).
pub(crate) fn selection_circle(tokens: &dyn Tokens, selected: bool) -> LayoutNode {
    let round = CornerRadius::uniform(RadiusToken::Full.px());
    if selected {
        let check = Icon::polylines(CHECK)
            .stroke_tenths(30)
            .size(13)
            .color(ColorToken::OnAccent)
            .build(tokens);
        let disc = VisualStyle {
            background: Some(tokens.color(ColorToken::Accent)),
            corner_radius: round,
            ..VisualStyle::default()
        };
        return fixed(CIRCLE, CIRCLE, disc, alloc::vec![check]);
    }
    let ring = VisualStyle {
        border: EdgeBorder::all(FxPx::new(2), fade(tokens.color(ColorToken::LabelTertiary), 75)),
        corner_radius: round,
        ..VisualStyle::default()
    };
    fixed(CIRCLE, CIRCLE, ring, alloc::vec![])
}

/// The trailing accent check of a `Checkmark`-style selectable row (16 px, stroke 2.6); an
/// unselected row keeps the slot empty so the line does not shift when it toggles.
pub(crate) fn trailing_check(tokens: &dyn Tokens, selected: bool) -> LayoutNode {
    if selected {
        Icon::polylines(CHECK).stroke_tenths(26).size(16).color(ColorToken::Accent).build(tokens)
    } else {
        fixed(16, 16, VisualStyle::default(), alloc::vec![])
    }
}

/// The disclosure chevron: `chevron-right` 16, stroke 2, `labelTertiary`.
pub(crate) fn chevron(tokens: &dyn Tokens) -> LayoutNode {
    Icon::lucide(LucideSymbol::ChevronRight).size(16).color(ColorToken::LabelTertiary).build(tokens)
}

/// The reorder handle: three 18 px lines in `labelTertiary` inside a 4 px pad that overlaps the
/// row's trailing padding (margin −4 −4 −4 0), so the line keeps its height.
pub(crate) fn reorder_handle(tokens: &dyn Tokens) -> LayoutNode {
    let glyph = Icon::polylines(HANDLE).size(18).color(ColorToken::LabelTertiary).build(tokens);
    let m = FxPx::new(-4);
    LayoutNode::Stack(
        Stack {
            direction: Direction::Row,
            padding: EdgeInsets::all(FxPx::new(4)),
            align: Align::Center,
            justify: Justify::Center,
            item: FlexItem {
                flex_shrink: 0,
                margin: EdgeInsets { top: m, right: m, bottom: m, left: FxPx::ZERO },
                ..FlexItem::default()
            },
            ..Stack::default()
        },
        VisualStyle {
            corner_radius: CornerRadius::uniform(RadiusToken::Sm.px()),
            ..VisualStyle::default()
        },
        alloc::vec![glyph],
    )
}
