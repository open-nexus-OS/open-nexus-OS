// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! CONTEXT: `SearchBar` — the design-system search field (design artifacts `SearchBar`): a
//! capsule on the control height of its size with the widget's own magnifier (or the field
//! spinner while loading), filter tokens, the input and the clear disc; optionally the cancel
//! text button beside it while the search is active, and a scope bar slot below. Resting
//! surface = the control fill (or the glass card layer); focus draws the 2 px outline in the
//! tint. A pure builder producing a `LayoutNode`; the inner input is the low-level
//! [`TextField`] primitive (the app owns the value). The suggestion list needs a floating
//! surface anchored to the field, which the layout cannot express yet (TASK-0353 P7 report).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the tests below; tests/ui_v10_goldens (searchbar); dsl runtime kit_widgets
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode,
    Position, Rgba8, Stack, VisualStyle,
};
use nexus_style::InteractionState;
use nexus_theme_tokens::{ColorToken, ControlSize, RadiusToken, TextStyleToken, Tokens};
use nexus_widget_text_field::field::{
    self, box_padding, focus_ring, glyph, FieldAppearance, FieldMetrics, FieldState,
    DISABLED_OPACITY, MAGNIFIER, RING_OFFSET, STROKE_2,
};
use nexus_widget_text_field::TextField;

/// The cancel button's label the design defaults to.
pub const DEFAULT_CANCEL: &str = "Abbrechen";

/// The input's floor while it shares the field with tokens (design `minWidth: 48`).
const INPUT_MIN_WIDTH: i32 = 48;

/// A filter token before the text (design `tokens`): a tinted capsule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchToken {
    pub label: String,
    /// Interaction id (a tap removes the token).
    pub id: Option<&'static str>,
}

impl SearchToken {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), id: None }
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }
}

/// A search field.
#[derive(Debug, Clone)]
pub struct SearchBar {
    value: String,
    placeholder: Option<String>,
    leading: Option<LayoutNode>,
    trailing: Option<LayoutNode>,
    state: InteractionState,
    id: Option<&'static str>,
    size: ControlSize,
    appearance: FieldAppearance,
    tint: ColorToken,
    clear_id: Option<&'static str>,
    cancel: Option<String>,
    cancel_id: Option<&'static str>,
    loading: bool,
    tokens: Vec<SearchToken>,
    token_selected: bool,
    scopes: Option<LayoutNode>,
}

impl Default for SearchBar {
    fn default() -> Self {
        Self {
            value: String::new(),
            placeholder: None,
            leading: None,
            trailing: None,
            state: InteractionState::Default,
            id: None,
            size: ControlSize::Md,
            appearance: FieldAppearance::Filled,
            tint: ColorToken::Accent,
            clear_id: None,
            cancel: None,
            cancel_id: None,
            loading: false,
            tokens: Vec::new(),
            token_selected: false,
            scopes: None,
        }
    }
}

impl SearchBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }
    /// Replaces the widget's magnifier with a caller node.
    pub fn leading(mut self, leading: LayoutNode) -> Self {
        self.leading = Some(leading);
        self
    }
    /// Replaces the widget's clear disc with a caller node (shown always).
    pub fn trailing(mut self, trailing: LayoutNode) -> Self {
        self.trailing = Some(trailing);
        self
    }
    pub fn state(mut self, state: InteractionState) -> Self {
        self.state = state;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }
    /// The control size (xs 24 · sm 30 · md 36 · lg 44 · xl 52).
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
    /// The field surface: filled (default) or the glass card layer on a wallpaper.
    pub fn appearance(mut self, appearance: FieldAppearance) -> Self {
        self.appearance = appearance;
        self
    }
    /// The tint (focus outline, tokens, cancel button; default `Accent`).
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = tint;
        self
    }
    /// Interaction id of the clear disc (the app empties the value).
    pub fn clear_id(mut self, id: &'static str) -> Self {
        self.clear_id = Some(id);
        self
    }
    /// Show the cancel text button while the search is active (focused, typed or filtered).
    pub fn cancel_button(mut self, label: impl Into<String>) -> Self {
        self.cancel = Some(label.into());
        self
    }
    /// Interaction id of the cancel button (the app clears the search and drops focus).
    pub fn cancel_id(mut self, id: &'static str) -> Self {
        self.cancel_id = Some(id);
        self
    }
    /// The field spinner in place of the magnifier (its rotation is host motion).
    pub fn loading(mut self, on: bool) -> Self {
        self.loading = on;
        self
    }
    /// Filter tokens before the text.
    pub fn tokens(mut self, tokens: Vec<SearchToken>) -> Self {
        self.tokens = tokens;
        self
    }
    /// The last token is selected (the first Backspace at the start): tint fill + `onTint`.
    pub fn token_selected(mut self, on: bool) -> Self {
        self.token_selected = on;
        self
    }
    /// The scope bar under the field (a `Segment`, size [`scope_size`](Self::scope_size)).
    pub fn scopes(mut self, scopes: LayoutNode) -> Self {
        self.scopes = Some(scopes);
        self
    }

    /// Whether the search is active (the cancel button shows).
    pub fn is_active(&self) -> bool {
        self.state.shows_focus_ring() || !self.value.is_empty() || !self.tokens.is_empty()
    }

    /// The size the scope bar uses: sm, md for lg/xl fields.
    pub fn scope_size(&self) -> ControlSize {
        if matches!(self.size, ControlSize::Lg | ControlSize::Xl) {
            ControlSize::Md
        } else {
            ControlSize::Sm
        }
    }

    /// A token: height `h − 12`, padding 0×8, capsule, the tint wash with the label one text
    /// step below the field text at weight 600 in the tint — selected: tint fill + `onTint`.
    fn token(
        &self,
        token: &SearchToken,
        selected: bool,
        m: &FieldMetrics,
        tokens: &dyn Tokens,
    ) -> LayoutNode {
        let tint = tokens.color(self.tint);
        let on_tint = tokens.color(on_tint(self.tint));
        let text = match self.size {
            ControlSize::Xs => TextStyleToken::Caption2,
            ControlSize::Sm => TextStyleToken::Caption,
            ControlSize::Md => TextStyleToken::Footnote,
            ControlSize::Lg => TextStyleToken::Subheadline,
            ControlSize::Xl => TextStyleToken::Body,
        };
        let style =
            text.text_style(if selected { on_tint } else { tint }, Some(FontWeight::Semibold));
        let h = Some(m.height - FxPx::new(12));
        LayoutNode::Stack(
            Stack {
                id: token.id,
                direction: Direction::Row,
                gap: FxPx::new(4),
                padding: EdgeInsets::symmetric(FxPx::ZERO, FxPx::new(8)),
                align: Align::Center,
                min_height: h,
                max_height: h,
                item: FlexItem { flex_shrink: 1, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle {
                background: Some(if selected { tint } else { field::wash(tint, 16) }),
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                ..VisualStyle::default()
            },
            alloc::vec![field::text_node(token.label.as_str(), style)],
        )
    }

    /// The cancel text button: field text w500 in the tint, padding 0×2, on the field height.
    fn cancel_button_node(&self, m: &FieldMetrics, tokens: &dyn Tokens) -> LayoutNode {
        let label = self.cancel.as_deref().unwrap_or(DEFAULT_CANCEL);
        let style = m.text.text_style(tokens.color(self.tint), Some(FontWeight::Medium));
        let h = Some(m.height);
        LayoutNode::Stack(
            Stack {
                id: self.cancel_id,
                direction: Direction::Row,
                padding: EdgeInsets::symmetric(FxPx::ZERO, FxPx::new(2)),
                align: Align::Center,
                min_height: h,
                max_height: h,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle::default(),
            alloc::vec![field::text_node(label, style)],
        )
    }

    /// The field capsule: magnifier/spinner · tokens · input · clear disc · focus outline.
    fn field(&self, m: &FieldMetrics, tokens: &dyn Tokens) -> LayoutNode {
        let disabled = self.state.is_disabled();
        let secondary = tokens.color(ColorToken::LabelSecondary);
        let mut kids: Vec<LayoutNode> = Vec::new();
        kids.push(match (&self.leading, self.loading) {
            (Some(leading), _) => leading.clone(),
            (None, true) => field_spinner(m.icon, secondary),
            (None, false) => glyph(MAGNIFIER, STROKE_2, m.icon, secondary),
        });
        let last = self.tokens.len().saturating_sub(1);
        for (i, token) in self.tokens.iter().enumerate() {
            kids.push(self.token(token, self.token_selected && i == last, m, tokens));
        }
        let mut tf = TextField::new()
            .text_style(field::input_style(m, tokens))
            .value(self.value.clone())
            .fill_row()
            .min_width(INPUT_MIN_WIDTH)
            .caret_color(tokens.color(self.tint));
        if self.tokens.is_empty() {
            if let Some(p) = &self.placeholder {
                tf = tf
                    .placeholder(p.clone())
                    .placeholder_color(tokens.color(ColorToken::LabelTertiary));
            }
        }
        if let Some(id) = self.id {
            tf = tf.id(id);
        }
        kids.push(tf.build());
        match &self.trailing {
            Some(trailing) => kids.push(trailing.clone()),
            None if !self.value.is_empty() && !disabled => {
                kids.push(field::clear_disc(m.clear_disc(), self.clear_id, tokens));
            }
            None => {}
        }
        let radius = RadiusToken::Full.px();
        let padding = box_padding(m.pad, FxPx::ZERO);
        if self.state.shows_focus_ring() {
            let ring = tokens.color(FieldState::Default.ring(self.tint));
            kids.push(focus_ring(ring, radius, padding, RING_OFFSET));
        }
        let style =
            field::surface(self.appearance, FieldState::Default, false, tokens).rounded(radius);
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: m.gap,
                padding,
                align: Align::Center,
                min_height: Some(m.height),
                max_height: Some(m.height),
                ..Stack::default()
            },
            style.visual(),
            kids,
        )
    }

    /// Build the search field — wrapped in a row with the cancel button while active, and in a
    /// column over the scope bar when one is set.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let m = FieldMetrics::of(self.size);
        let dim = self.state.is_disabled().then_some(DISABLED_OPACITY);
        let show_cancel = self.cancel.is_some() && self.is_active() && dim.is_none();
        let mut top = self.field(&m, tokens);
        if show_cancel {
            if let LayoutNode::Stack(s, _, _) = &mut top {
                s.item.flex_grow = 1;
            }
            let cancel = self.cancel_button_node(&m, tokens);
            top = LayoutNode::Stack(
                Stack {
                    direction: Direction::Row,
                    gap: FxPx::new(12),
                    align: Align::Center,
                    ..Stack::default()
                },
                VisualStyle::default(),
                alloc::vec![top, cancel],
            );
        }
        let Some(mut scopes) = self.scopes else {
            if let LayoutNode::Stack(_, v, _) = &mut top {
                v.opacity = dim;
            }
            return top;
        };
        if let LayoutNode::Stack(s, _, _) = &mut scopes {
            s.item.align_self = Some(Align::Start);
        }
        LayoutNode::Stack(
            Stack {
                direction: Direction::Column,
                gap: FxPx::new(8),
                align: Align::Stretch,
                ..Stack::default()
            },
            VisualStyle { opacity: dim, ..VisualStyle::default() },
            alloc::vec![top, scopes],
        )
    }
}

/// Text and icons on a filled tint: `onPrimary` on `primary`/`label`, `onAccent` otherwise.
fn on_tint(tint: ColorToken) -> ColorToken {
    match tint {
        ColorToken::Primary | ColorToken::Label => ColorToken::OnPrimary,
        _ => ColorToken::OnAccent,
    }
}

/// The field spinner (design `FieldSpinner`): 12 rounded spokes from the edge inward, width
/// `max(1.5, 0.1 × size)`, length `0.27 × size`, spoke `i` at opacity `.25 + .75 × (i+1)/12`.
/// A static frame — the steps rotation is host motion.
fn field_spinner(size: i32, color: Rgba8) -> LayoutNode {
    // In thousandths of the box: spoke width, and the inner / outer end of its centre line.
    let w = (size * 100).max(1500) / size; // max(1.5 px, 10 %) of the box, in milli
    let len = 270;
    let (r_out, r_in) = (500 - w / 2, 500 - (len - w / 2));
    let d = Some(FxPx::new(size));
    let spokes = (0..12)
        .map(|i| {
            let (sin, cos) = SPOKE_DIR[i];
            let at = |r: i32| {
                let x = 500 + r * i32::from(sin) / 10_000;
                let y = 500 - r * i32::from(cos) / 10_000;
                (x.clamp(0, 1000) as u16, y.clamp(0, 1000) as u16)
            };
            let alpha = 64 + 191 * (i as u32 + 1) / 12;
            let tint =
                Rgba8::new(color.r, color.g, color.b, (u32::from(color.a) * alpha / 255) as u8);
            let mut spoke = glyph(&[&[at(r_in), at(r_out)]], w as u16, size, tint);
            if let LayoutNode::Stack(s, _, _) = &mut spoke {
                s.item.position = Position::Absolute;
            }
            spoke
        })
        .collect();
    LayoutNode::Stack(
        Stack {
            justify: Justify::Center,
            min_width: d,
            max_width: d,
            min_height: d,
            max_height: d,
            item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
            ..Stack::default()
        },
        VisualStyle::default(),
        spokes,
    )
}

/// `(sin, cos) × 10 000` of the twelve spoke directions, clockwise from 12 o'clock.
const SPOKE_DIR: [(i16, i16); 12] = [
    (0, 10000),
    (5000, 8660),
    (8660, 5000),
    (10000, 0),
    (8660, -5000),
    (5000, -8660),
    (0, -10000),
    (-5000, -8660),
    (-8660, -5000),
    (-10000, 0),
    (-8660, 5000),
    (-5000, 8660),
];

#[cfg(test)]
mod tests;
