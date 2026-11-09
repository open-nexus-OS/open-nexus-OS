// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `Segment` — the design-system segmented control (the design artifacts' `Segment`): a
//! capsule track of the control height in `fill` with the glass edge, inset 2 (4 at lg/xl),
//! holding one item per option; the active item carries the thumb — a raised `surfaceOverlay`
//! capsule with `shadow1` — and a semibold label in `label` (the tint when one is set), the
//! others a medium label in `labelSecondary`. Items are h − 2·inset − 2 tall, padded 0 × px
//! (8/8/12/16/20; icon-only 0 × 8, min width item height + 6).
//!
//! A pure builder producing a `LayoutNode::Stack` row. Options are either labels (the widget
//! sets them) or caller-built nodes (they own their content and ids), so the app maps the
//! tapped option → value. Hover never repaints it; a disabled control dims to 0.45, a disabled
//! option to 0.40. DSL-emittable.

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, Stack,
    TextContent, TextNode, TextStyle, VisualStyle, WhiteSpace,
};
use nexus_style::control::metrics;
use nexus_style::{Dim, InteractionState, Style};
use nexus_theme_tokens::{
    ColorToken, ControlSize, RadiusToken, ShadowToken, TextStyleToken, Tokens,
};

/// Legacy size preset; maps onto the control sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegmentSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl From<SegmentSize> for ControlSize {
    fn from(size: SegmentSize) -> Self {
        match size {
            SegmentSize::Sm => ControlSize::Sm,
            SegmentSize::Md => ControlSize::Md,
            SegmentSize::Lg => ControlSize::Lg,
        }
    }
}

/// The track geometry of a size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentMetrics {
    /// The track's inset around the items.
    pub pad: FxPx,
    /// The gap between items.
    pub gap: FxPx,
    /// The item's horizontal padding.
    pub item_pad_x: FxPx,
    /// The item height (h − 2·pad − 2: inside the 1 px edge).
    pub item_height: FxPx,
    /// The label's text style.
    pub text: TextStyleToken,
}

impl SegmentMetrics {
    /// xs 2/0/8 caption · sm 2/2/8 footnote · md 2/2/12 subheadline · lg 4/2/16 headline ·
    /// xl 4/2/20 title3 (pad / gap / item padding / label).
    #[must_use]
    pub fn of(size: ControlSize) -> Self {
        let (pad, gap, px, text) = match size {
            ControlSize::Xs => (2, 0, 8, TextStyleToken::Caption),
            ControlSize::Sm => (2, 2, 8, TextStyleToken::Footnote),
            ControlSize::Md => (2, 2, 12, TextStyleToken::Subheadline),
            ControlSize::Lg => (4, 2, 16, TextStyleToken::Headline),
            ControlSize::Xl => (4, 2, 20, TextStyleToken::Title3),
        };
        SegmentMetrics {
            pad: FxPx::new(pad),
            gap: FxPx::new(gap),
            item_pad_x: FxPx::new(px),
            item_height: FxPx::new(size.height().0 - 2 * pad - 2),
            text,
        }
    }
}

#[derive(Debug, Clone)]
enum Item {
    Node(Box<LayoutNode>),
    Label(String),
}

/// A segmented control.
#[derive(Debug, Clone, Default)]
pub struct Segment {
    active: usize,
    size: ControlSize,
    id: Option<&'static str>,
    items: Vec<Item>,
    icon_only: bool,
    tint: Option<ColorToken>,
    state: InteractionState,
    disabled: Vec<usize>,
}

impl Segment {
    pub fn new() -> Self {
        Self::default()
    }

    /// Index of the active option (it carries the thumb).
    pub fn active(mut self, active: usize) -> Self {
        self.active = active;
        self
    }

    /// The control size (a [`SegmentSize`] maps onto it).
    pub fn size(mut self, size: impl Into<ControlSize>) -> Self {
        self.size = size.into();
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Caller-built option nodes (they own their content and ids).
    pub fn options(mut self, options: Vec<LayoutNode>) -> Self {
        self.items = options.into_iter().map(|node| Item::Node(Box::new(node))).collect();
        self
    }

    /// Text options, set by the widget in the size's label style.
    pub fn labels<S: Into<String>>(mut self, labels: impl IntoIterator<Item = S>) -> Self {
        self.items = labels.into_iter().map(|l| Item::Label(l.into())).collect();
        self
    }

    /// Caller-built options are icons: items padded 0 × 8, at least item height + 6 wide.
    pub fn icon_only(mut self, icon_only: bool) -> Self {
        self.icon_only = icon_only;
        self
    }

    /// The active label's colour role (the thumb stays neutral).
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = Some(tint);
        self
    }

    pub fn state(mut self, state: InteractionState) -> Self {
        self.state = state;
        self
    }

    /// Disable one option (dims it to 0.40).
    pub fn disable_option(mut self, index: usize) -> Self {
        self.disabled.push(index);
        self
    }

    /// The label colour role of an item: `label` (or the tint) when active, `labelSecondary`
    /// otherwise — for callers colouring their own option nodes.
    pub fn label_role(&self, active: bool) -> ColorToken {
        match (active, self.tint) {
            (true, Some(tint)) => tint,
            (true, None) => ColorToken::Label,
            (false, _) => ColorToken::LabelSecondary,
        }
    }

    fn label_node(&self, tokens: &dyn Tokens, text: String, active: bool) -> LayoutNode {
        let m = SegmentMetrics::of(self.size);
        let weight = if active { FontWeight::Semibold } else { FontWeight::Medium };
        let style = TextStyle {
            white_space: WhiteSpace::NoWrap,
            ..m.text.text_style(tokens.color(self.label_role(active)), Some(weight))
        };
        LayoutNode::Text(
            TextNode {
                content: TextContent::new(text),
                style,
                max_lines: Some(1),
                ..TextNode::default()
            },
            VisualStyle::default(),
        )
    }

    fn item(&self, tokens: &dyn Tokens, index: usize, item: Item) -> LayoutNode {
        let m = SegmentMetrics::of(self.size);
        let active = index == self.active;
        let mut style = Style::new().rounded_token(RadiusToken::Full);
        if active {
            // The thumb: a raised opaque capsule behind the active option.
            style = style
                .background_token(tokens, ColorToken::SurfaceOverlay)
                .shadow_token(tokens, ShadowToken::Shadow1);
        }
        if self.disabled.contains(&index) && !self.state.is_disabled() {
            style = style.opacity(Dim::Option.opacity());
        }
        let icon = self.icon_only && matches!(item, Item::Node(_));
        let child = match item {
            Item::Node(node) => *node,
            Item::Label(text) => self.label_node(tokens, text, active),
        };
        let h = Some(m.item_height);
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                gap: metrics(self.size).gap,
                padding: EdgeInsets::symmetric(
                    FxPx::ZERO,
                    if icon { FxPx::new(8) } else { m.item_pad_x },
                ),
                align: Align::Center,
                justify: Justify::Center,
                min_width: icon.then(|| FxPx::new(m.item_height.0 + 6)),
                min_height: h,
                max_height: h,
                ..Stack::default()
            },
            style.visual(),
            alloc::vec![child],
        )
    }

    /// Build the segmented control node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let m = SegmentMetrics::of(self.size);
        let mut track = Style::new()
            .background_token(tokens, ColorToken::Fill)
            .border_token(tokens, ColorToken::GlassEdge)
            .rounded_token(RadiusToken::Full);
        if self.state.shows_focus_ring() {
            track = track.focus_ring(tokens, ColorToken::Accent);
        }
        if self.state.is_disabled() {
            track = track.opacity(self.state.opacity_as(Dim::Control));
        }
        let items: Vec<LayoutNode> = self
            .items
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, item)| self.item(tokens, i, item))
            .collect();
        let h = Some(self.size.height());
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: m.gap,
                // The inset plus the 1 px edge (an edge takes no layout space here).
                padding: EdgeInsets::all(FxPx::new(m.pad.0 + 1)),
                align: Align::Center,
                min_height: h,
                max_height: h,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            track.visual(),
            items,
        )
    }
}

#[cfg(test)]
mod tests;
