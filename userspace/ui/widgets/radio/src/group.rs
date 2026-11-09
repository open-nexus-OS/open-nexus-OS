// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `RadioGroup` — the single-select list of the design artifacts' `GlassRadioGroup`: a column
//! (or a row) of option rows, gap 4. A row is the control height tall (min), padded 0 × padX
//! (8/8/12/12/16), radius 8 (xs/sm) or 12, and holds the [`Radio`] indicator and its label
//! (the control label, gap 8/8/12/12/12); the selected row carries the persistent `fill` layer.
//! A disabled option dims to 0.45, a disabled group too. Hover is the host's layer.

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, FlexItem, FxPx, LayoutNode, Stack, TextContent, TextNode, VisualStyle,
    WhiteSpace,
};
use nexus_style::control::{label_style, metrics};
use nexus_style::{Dim, InteractionState, Style};
use nexus_theme_tokens::{ColorToken, ControlSize, RadiusToken, Tokens};

use crate::Radio;

/// One option of a [`RadioGroup`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadioOption {
    label: String,
    id: Option<&'static str>,
    disabled: bool,
}

impl RadioOption {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), id: None, disabled: false }
    }

    /// The row's interaction id.
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// How the rows run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    /// A list.
    #[default]
    Vertical,
    /// An inline row (labels stay on one line).
    Horizontal,
}

/// A single-select radio list.
#[derive(Debug, Clone, Default)]
pub struct RadioGroup {
    options: Vec<RadioOption>,
    selected: Option<usize>,
    orientation: Orientation,
    size: ControlSize,
    tint: Option<ColorToken>,
    state: InteractionState,
    id: Option<&'static str>,
}

impl RadioGroup {
    pub fn new(options: Vec<RadioOption>) -> Self {
        Self { options, ..Self::default() }
    }

    /// The selected option's index.
    pub fn selected(mut self, index: usize) -> Self {
        self.selected = Some(index);
        self
    }

    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    /// The selected radio's colour role (default accent).
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = Some(tint);
        self
    }

    /// The group's state (disabled dims the whole group).
    pub fn state(mut self, state: InteractionState) -> Self {
        self.state = state;
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The row padding, the radio↔label gap and the row radius of the size.
    fn row_metrics(&self) -> (FxPx, FxPx, RadiusToken) {
        match self.size {
            ControlSize::Xs | ControlSize::Sm => (FxPx::new(8), FxPx::new(8), RadiusToken::Sm),
            ControlSize::Md | ControlSize::Lg => (FxPx::new(12), FxPx::new(12), RadiusToken::Md),
            ControlSize::Xl => (FxPx::new(16), FxPx::new(12), RadiusToken::Md),
        }
    }

    fn row(&self, tokens: &dyn Tokens, index: usize, option: &RadioOption) -> LayoutNode {
        let (pad, gap, radius) = self.row_metrics();
        let on = self.selected == Some(index);
        let mut style = Style::new().rounded_token(radius);
        if on {
            // The persistent selected layer.
            style = style.background_token(tokens, ColorToken::Fill);
        }
        if option.disabled && !self.state.is_disabled() {
            style = style.opacity(Dim::Control.opacity());
        }
        let mut radio = Radio::new().selected(on).size(self.size);
        if let Some(tint) = self.tint {
            radio = radio.tint(tint);
        }
        let mut label = label_style(self.size, tokens.color(ColorToken::Label));
        if self.orientation == Orientation::Vertical {
            label.white_space = WhiteSpace::Normal;
        }
        let text = LayoutNode::Text(
            TextNode {
                content: TextContent::new(option.label.clone()),
                style: label,
                ..TextNode::default()
            },
            VisualStyle::default(),
        );
        LayoutNode::Stack(
            Stack {
                id: option.id,
                direction: Direction::Row,
                gap,
                padding: nexus_layout_types::EdgeInsets::symmetric(FxPx::ZERO, pad),
                align: Align::Center,
                min_height: Some(metrics(self.size).height),
                ..Stack::default()
            },
            style.visual(),
            alloc::vec![radio.build(tokens), text],
        )
    }

    /// Build the group node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let rows = self.options.iter().enumerate().map(|(i, o)| self.row(tokens, i, o)).collect();
        let mut style = Style::new();
        if self.state.is_disabled() {
            style = style.opacity(self.state.opacity_as(Dim::Control));
        }
        let vertical = self.orientation == Orientation::Vertical;
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: if vertical { Direction::Column } else { Direction::Row },
                gap: FxPx::new(4),
                align: if vertical { Align::Stretch } else { Align::Center },
                item: FlexItem::default(),
                ..Stack::default()
            },
            style.visual(),
            rows,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::Fraction;
    use nexus_theme_tokens::BaseTokens;

    fn group() -> RadioGroup {
        RadioGroup::new(alloc::vec![
            RadioOption::new("Hell").id("light"),
            RadioOption::new("Dunkel").id("dark"),
            RadioOption::new("Automatisch").disabled(true),
        ])
    }

    fn rows(node: &LayoutNode) -> &[LayoutNode] {
        match node {
            LayoutNode::Stack(_, _, rows) => rows,
            _ => panic!("a group is a Stack"),
        }
    }

    fn row_parts(node: &LayoutNode) -> (&Stack, &VisualStyle, &[LayoutNode]) {
        match node {
            LayoutNode::Stack(s, v, c) => (s, v, c),
            _ => panic!("a row is a Stack"),
        }
    }

    #[test]
    fn rows_are_the_control_height_with_the_size_padding() {
        let t = BaseTokens;
        for (size, h, pad, radius) in [
            (ControlSize::Xs, 24, 8, 8),
            (ControlSize::Md, 36, 12, 12),
            (ControlSize::Xl, 52, 16, 12),
        ] {
            let node = group().size(size).build(&t);
            let (stack, v, children) = row_parts(&rows(&node)[0]);
            assert_eq!(stack.min_height, Some(FxPx::new(h)), "{size:?}");
            assert_eq!(stack.padding.left, FxPx::new(pad), "{size:?}");
            assert_eq!(v.corner_radius.top_left, FxPx::new(radius), "{size:?}");
            assert_eq!(stack.id, Some("light"));
            assert_eq!(children.len(), 2, "radio + label");
        }
    }

    #[test]
    fn the_selected_row_carries_the_fill_layer_and_the_selected_radio() {
        let t = BaseTokens;
        let node = group().selected(1).build(&t);
        let (_, off, _) = row_parts(&rows(&node)[0]);
        let (_, on, children) = row_parts(&rows(&node)[1]);
        assert_eq!(off.background, None);
        assert_eq!(on.background, Some(t.color(ColorToken::Fill)));
        let (_, radio, dot) = row_parts(&children[0]);
        assert_eq!(radio.background, Some(t.color(ColorToken::Accent)));
        assert_eq!(dot.len(), 1);
    }

    #[test]
    fn disabled_options_and_groups_dim_to_045() {
        let t = BaseTokens;
        let node = group().build(&t);
        assert_eq!(row_parts(&rows(&node)[2]).1.opacity, Some(Fraction::new(115)));
        let off = group().state(InteractionState::Disabled).build(&t);
        assert_eq!(row_parts(&off).1.opacity, Some(Fraction::new(115)));
        assert_eq!(row_parts(&rows(&off)[2]).1.opacity, None, "not twice");
    }

    #[test]
    fn horizontal_runs_the_rows_in_a_row() {
        let t = BaseTokens;
        let node = group().orientation(Orientation::Horizontal).build(&t);
        let (stack, _, _) = row_parts(&node);
        assert_eq!(stack.direction, Direction::Row);
        assert_eq!(stack.gap, FxPx::new(4));
    }
}
