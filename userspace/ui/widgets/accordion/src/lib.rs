// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `Accordion` — the design artifacts' disclosure group (design `Accordion`, RFC-0116): a card
//! (`glassCard`, 1 px `glassEdge`, radius 16, clipped) of sections separated by hairlines. A
//! section header is a 44 px full-width row (12 16 padding, 12 gap): an optional `label` icon,
//! the title in body semibold `label`, and a 16 px `chevron-down` (stroke 2.2) in
//! `labelSecondary` that turns over when the section is open. Open content pads 0 16 16. On an
//! opaque pane the card re-tints to `fill` at 50 %. Pure builder producing a `LayoutNode::Stack`
//! column; the open/close motion and its chevron turn are the runtime's.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Border, CornerRadius, Direction, EdgeBorder, EdgeInsets, FlexItem, FontWeight, FxPx,
    LayoutNode, Material, Overflow, Rgba8, Stack, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, RadiusToken, Tokens, HAIRLINE};
use nexus_widget_icon::{with_ink, Icon, LucideSymbol};
use nexus_widget_text::Text;

/// One collapsible section.
#[derive(Debug, Clone)]
pub struct AccordionItem {
    title: String,
    content: LayoutNode,
    open: bool,
    icon: Option<LayoutNode>,
    id: Option<&'static str>,
}

impl AccordionItem {
    pub fn new(title: impl Into<String>, content: LayoutNode) -> Self {
        Self { title: title.into(), content, open: false, icon: None, id: None }
    }
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }
    /// A leading icon in the header; the header inks it `label`.
    pub fn icon(mut self, icon: LayoutNode) -> Self {
        self.icon = Some(icon);
        self
    }
    /// The header's node id (its tap target).
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }
}

/// A disclosure group.
#[derive(Debug, Clone, Default)]
pub struct Accordion {
    items: Vec<AccordionItem>,
    on_pane: bool,
    id: Option<&'static str>,
}

impl Accordion {
    pub fn new(items: Vec<AccordionItem>) -> Self {
        Self { items, on_pane: false, id: None }
    }
    /// The card sits on an opaque pane or overlay: it takes `fill` at 50 % instead of
    /// `glassCard` (which would vanish there).
    pub fn on_pane(mut self, on_pane: bool) -> Self {
        self.on_pane = on_pane;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The disclosure chevron: down when closed, turned over when open.
    fn chevron(tokens: &dyn Tokens, open: bool) -> LayoutNode {
        let symbol = if open { LucideSymbol::ChevronUp } else { LucideSymbol::ChevronDown };
        Icon::lucide(symbol)
            .stroke_tenths(22)
            .size(16)
            .color(ColorToken::LabelSecondary)
            .build(tokens)
    }

    fn section(tokens: &dyn Tokens, item: AccordionItem, first: bool) -> LayoutNode {
        let mut header: Vec<LayoutNode> = Vec::new();
        if let Some(icon) = item.icon {
            header.push(with_ink(icon, tokens.color(ColorToken::Label)));
        }
        let mut title = Text::new(item.title).weight(FontWeight::Semibold).build(tokens);
        if let LayoutNode::Text(t, _) = &mut title {
            t.item.flex_grow = 1;
        }
        header.push(title);
        header.push(Self::chevron(tokens, item.open));
        let header = LayoutNode::Stack(
            Stack {
                id: item.id,
                direction: Direction::Row,
                gap: FxPx::new(12),
                padding: EdgeInsets::symmetric(FxPx::new(12), FxPx::new(16)),
                align: Align::Center,
                ..Stack::default()
            },
            VisualStyle::default(),
            header,
        );

        let mut children = alloc::vec![header];
        if item.open {
            children.push(LayoutNode::Stack(
                Stack {
                    direction: Direction::Column,
                    padding: EdgeInsets {
                        top: FxPx::ZERO,
                        right: FxPx::new(16),
                        bottom: FxPx::new(16),
                        left: FxPx::new(16),
                    },
                    align: Align::Stretch,
                    ..Stack::default()
                },
                VisualStyle::default(),
                alloc::vec![item.content],
            ));
        }
        let border = if first {
            EdgeBorder::none()
        } else {
            EdgeBorder {
                top: Some(Border { width: HAIRLINE, color: tokens.color(ColorToken::Separator) }),
                ..EdgeBorder::none()
            }
        };
        LayoutNode::Stack(
            Stack { direction: Direction::Column, align: Align::Stretch, ..Stack::default() },
            VisualStyle { border, ..VisualStyle::default() },
            children,
        )
    }

    /// Build the accordion node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        // RFC-0116: a grouped container is the card material (translucent, glass edge).
        let mut visual = Style::new().material(Material::Card, tokens).visual();
        visual.corner_radius = CornerRadius::uniform(RadiusToken::Lg.px());
        if self.on_pane {
            let fill = tokens.color(ColorToken::Fill);
            visual.background = Some(Rgba8 { a: fill.a / 2, ..fill });
        }
        let sections: Vec<LayoutNode> = self
            .items
            .into_iter()
            .enumerate()
            .map(|(i, item)| Self::section(tokens, item, i == 0))
            .collect();
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Column,
                // The design's 1 px edge sits outside the sections; the painter draws borders
                // inside the box, so the padding carries that pixel.
                padding: EdgeInsets::all(HAIRLINE),
                align: Align::Stretch,
                overflow: Overflow::Hidden,
                item: FlexItem::default(),
                ..Stack::default()
            },
            visual,
            sections,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout::LayoutEngine;
    use nexus_layout_types::ShapeKind;
    use nexus_text_baked::measure_text::BakedTextMeasure;
    use nexus_theme_tokens::BaseTokens;

    fn body() -> LayoutNode {
        LayoutNode::Stack(
            Stack {
                min_height: Some(FxPx::new(20)),
                max_height: Some(FxPx::new(20)),
                ..Stack::default()
            },
            VisualStyle::default(),
            alloc::vec![],
        )
    }

    fn acc(t: &dyn Tokens) -> LayoutNode {
        Accordion::new(alloc::vec![
            AccordionItem::new("Allgemein", body()).open(true).id("a"),
            AccordionItem::new("Datenschutz", body()).id("b"),
        ])
        .id("acc")
        .build(t)
    }

    #[test]
    fn a_card_of_sections_with_hairlines_between() {
        let t = BaseTokens;
        match acc(&t) {
            LayoutNode::Stack(stack, v, sections) => {
                assert_eq!(stack.id, Some("acc"));
                assert_eq!(stack.min_width, None, "no minimum width");
                assert_eq!(stack.overflow, Overflow::Hidden);
                assert_eq!(v.background, Some(t.color(ColorToken::GlassCard)));
                assert_eq!(v.material, Some(Material::Card));
                assert_eq!(v.border.top.map(|b| b.color), Some(t.color(ColorToken::GlassEdge)));
                assert_eq!(v.corner_radius.top_left, FxPx::new(16));
                let kids = |n: &LayoutNode| match n {
                    LayoutNode::Stack(_, _, c) => c.len(),
                    _ => 0,
                };
                assert_eq!(kids(&sections[0]), 2, "open reveals content");
                assert_eq!(kids(&sections[1]), 1, "closed is header only");
                match (&sections[0], &sections[1]) {
                    (LayoutNode::Stack(_, first, _), LayoutNode::Stack(_, second, _)) => {
                        assert_eq!(first.border.top, None);
                        assert_eq!(
                            second.border.top.map(|b| b.color),
                            Some(t.color(ColorToken::Separator))
                        );
                    }
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
    }

    #[test]
    fn headers_are_44_and_content_pads_0_16_16() {
        let t = BaseTokens;
        let all = LayoutEngine::new()
            .layout(&acc(&t), FxPx::new(360), &BakedTextMeasure)
            .expect("layout")
            .boxes;
        let a = all.iter().find(|b| b.id == Some("a")).expect("header a");
        let b = all.iter().find(|b| b.id == Some("b")).expect("header b");
        assert_eq!(a.rect.height, FxPx::new(44));
        assert_eq!((a.rect.x, a.rect.y), (FxPx::new(1), FxPx::new(1)));
        // 1 + header 44 + content 20 + 16 → the second header.
        assert_eq!(b.rect.y, FxPx::new(1 + 44 + 20 + 16));
        assert_eq!(all[0].rect.height, FxPx::new(1 + 44 + 36 + 44 + 1));
    }

    #[test]
    fn title_is_body_semibold_and_the_chevron_turns() {
        let t = BaseTokens;
        let LayoutNode::Stack(_, _, sections) = acc(&t) else { panic!() };
        let header = |s: &LayoutNode| match s {
            LayoutNode::Stack(_, _, c) => match &c[0] {
                LayoutNode::Stack(_, _, h) => h.clone(),
                _ => panic!(),
            },
            _ => panic!(),
        };
        let open = header(&sections[0]);
        match (&open[0], &open[1]) {
            (LayoutNode::Text(n, _), LayoutNode::Stack(cs, cv, _)) => {
                assert_eq!((n.style.font_size.0, n.style.font_weight), (14, FontWeight::Semibold));
                assert_eq!(cs.min_width, Some(FxPx::new(16)));
                assert_eq!(cv.background, Some(t.color(ColorToken::LabelSecondary)));
                assert!(matches!(cv.shape, ShapeKind::Stroke { width_milli: 92, .. }));
            }
            _ => panic!(),
        }
        let closed = header(&sections[1]);
        assert_ne!(open[1], closed[1], "open and closed chevrons differ");
    }

    #[test]
    fn on_a_pane_the_card_retints() {
        let t = BaseTokens;
        let node =
            Accordion::new(alloc::vec![AccordionItem::new("A", body())]).on_pane(true).build(&t);
        match node {
            LayoutNode::Stack(_, v, _) => {
                let fill = t.color(ColorToken::Fill);
                assert_eq!(v.background, Some(Rgba8 { a: fill.a / 2, ..fill }));
            }
            _ => panic!(),
        }
    }
}
