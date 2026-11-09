// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `GlassButton` — the design-system button (the design artifacts' `GlassButton`): five looks
//! (glass by default, ghost, secondary, tinted, filled — plus the legacy names) × five control
//! sizes × the shape its role gives it: a capsule with a label, a circle when icon-only, the
//! size's rounded rect only on request. The widget owns its label (the size's control label,
//! weight 500, in the look's colour) and its geometry: one outer height per size, padding
//! 0 × padX, the icon↔label gap of the size.
//!
//! Hover and press never repaint the button — the host shows its layer over it; what the state
//! does draw is the disabled dim (0.50) and the focus ring. A pure builder producing the styled
//! container `LayoutNode` (delegating the box to the low-level [`Button`] primitive).
//!
//! Tree: root (the button box, the interaction id) → content (child 0) → [leading icon] label
//! [children…] [trailing icon]. With `.label(..)` and no leading icon the label is content child
//! 0 and declared children follow from 1 (the DSL's `child_path`); a legacy `.content(node)` is
//! child 0 itself.

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, Position, Rgba8,
    Stack, TextContent, TextNode, TextStyle, VisualStyle, WhiteSpace,
};
use nexus_style::control::{label_style, metrics, Role};
use nexus_style::{glyph, Dim, InteractionState, Style};
use nexus_theme_tokens::{
    ColorToken, ControlSize, RadiusToken, ShadowToken, TextStyleToken, Tokens,
};

use crate::look::{resolve, ButtonVariant, Look, LookInput};
use crate::Button;

/// The button size: the five control sizes plus the legacy icon size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    /// 24 px, caption label.
    Xs,
    /// 30 px, footnote label.
    Sm,
    /// 36 px, body label — the default.
    #[default]
    Md,
    /// 44 px, headline label.
    Lg,
    /// 52 px, title3 label.
    Xl,
    /// Legacy: an icon-only circle at md.
    Icon,
}

impl ButtonSize {
    /// The control size it draws at (`Icon` is md).
    #[must_use]
    pub const fn control(self) -> ControlSize {
        match self {
            ButtonSize::Xs => ControlSize::Xs,
            ButtonSize::Sm => ControlSize::Sm,
            ButtonSize::Md | ButtonSize::Icon => ControlSize::Md,
            ButtonSize::Lg => ControlSize::Lg,
            ButtonSize::Xl => ControlSize::Xl,
        }
    }

    /// The horizontal padding of a labelled button (the size's padX; an icon button has none).
    /// A button is the control height tall, so it has no vertical padding.
    #[must_use]
    pub fn padding(self) -> FxPx {
        match self {
            ButtonSize::Icon => FxPx::ZERO,
            size => metrics(size.control()).pad_x,
        }
    }

    /// The shape radius: every size is a capsule (or a circle) unless the shape asks otherwise.
    #[must_use]
    pub const fn radius(self) -> RadiusToken {
        RadiusToken::Full
    }
}

impl From<ControlSize> for ButtonSize {
    fn from(size: ControlSize) -> Self {
        match size {
            ControlSize::Xs => ButtonSize::Xs,
            ControlSize::Sm => ButtonSize::Sm,
            ControlSize::Md => ButtonSize::Md,
            ControlSize::Lg => ButtonSize::Lg,
            ControlSize::Xl => ButtonSize::Xl,
        }
    }
}

/// The shape. It follows the role: a labelled button is a capsule, an icon-only one a circle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonShape {
    /// By role (capsule, or a circle when icon-only).
    #[default]
    Auto,
    /// The size's rounded rect (8/8/12/12/16) — only for tiles in a rectangular bar.
    Rounded,
    /// A circle (width = height).
    Circle,
}

/// Where the icon sits next to the label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconPosition {
    #[default]
    Leading,
    Trailing,
    /// Stacked over the label — a tile (column, one text style smaller, radius lg).
    Top,
}

/// The design-system button. Build with [`GlassButton::build`] (needs the theme).
#[derive(Debug, Clone, Default)]
pub struct GlassButton {
    variant: Option<ButtonVariant>,
    prominent: bool,
    size: ButtonSize,
    shape: ButtonShape,
    tint: Option<ColorToken>,
    role: Option<Role>,
    state: InteractionState,
    selected: bool,
    full_width: bool,
    icon: Option<LayoutNode>,
    icon_position: IconPosition,
    label: Option<String>,
    children: Vec<LayoutNode>,
    content: Option<LayoutNode>,
    id: Option<&'static str>,
}

/// The ✕ of a close button (`M6 6l12 12M18 6L6 18`, stroke 2.6).
const GLYPH_CLOSE: [&[glyph::GridPoint]; 2] = [&[(60, 60), (180, 180)], &[(180, 60), (60, 180)]];
/// The ✓ of a confirm button (`M4.5 12.5l5 5L19.5 7`, stroke 2.6).
const GLYPH_CHECK: [&[glyph::GridPoint]; 1] = [&[(45, 125), (95, 175), (195, 70)]];

impl GlassButton {
    pub fn new() -> Self {
        Self::default()
    }

    /// The look (default glass; the filled look for `Role::Confirm`).
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = Some(variant);
        self
    }

    /// Turn any look into the filled one (the design's `prominent`).
    pub fn prominent(mut self, prominent: bool) -> Self {
        self.prominent = prominent;
        self
    }

    /// The control size (xs…xl, or the legacy `ButtonSize::Icon`).
    pub fn size(mut self, size: impl Into<ButtonSize>) -> Self {
        self.size = size.into();
        self
    }

    pub fn shape(mut self, shape: ButtonShape) -> Self {
        self.shape = shape;
        self
    }

    /// The tint role: the fill of the filled look, the wash of the tinted one, the label of the
    /// quiet ones (default accent).
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = Some(tint);
        self
    }

    /// The role: destructive tints `danger`, confirm fills (✓ circle when empty), close is a ✕
    /// circle when empty.
    pub fn role(mut self, role: Role) -> Self {
        self.role = Some(role);
        self
    }

    pub fn state(mut self, state: InteractionState) -> Self {
        self.state = state;
        self
    }

    /// The selected state of a toggle button: the persistent `fill` layer, a tinted quiet label.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Stretch across the container (a column's width); the height stays the control height.
    pub fn full_width(mut self, full_width: bool) -> Self {
        self.full_width = full_width;
        self
    }

    /// The icon node — built by the caller in [`Self::foreground_role`] at [`Self::icon_size`].
    pub fn icon(mut self, icon: LayoutNode) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn icon_position(mut self, position: IconPosition) -> Self {
        self.icon_position = position;
        self
    }

    /// The label (the widget sets it in the size's control label and the look's colour).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Nodes following the label inside the content row.
    pub fn children(mut self, children: Vec<LayoutNode>) -> Self {
        self.children = children;
        self
    }

    /// Legacy: a caller-built content node, colored with [`Self::foreground`]; it replaces the
    /// widget's own content (label, icon, children).
    pub fn content(mut self, content: LayoutNode) -> Self {
        self.content = Some(content);
        self
    }

    /// Interaction id (the compositor hit-tests the rendered rect by this id).
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    fn look(&self, tokens: &dyn Tokens) -> Look {
        resolve(
            LookInput {
                variant: self.variant,
                prominent: self.prominent,
                tint: self.tint,
                role: self.role,
                selected: self.selected,
            },
            tokens,
        )
    }

    /// The label and icon colour role of the resolved look.
    pub fn foreground_role(&self, tokens: &dyn Tokens) -> ColorToken {
        self.look(tokens).label
    }

    /// The resolved foreground (label/icon) colour.
    pub fn foreground(&self, tokens: &dyn Tokens) -> Rgba8 {
        tokens.color(self.foreground_role(tokens))
    }

    /// The icon size of the button's control size.
    pub fn icon_size(&self) -> FxPx {
        metrics(self.size.control()).icon
    }

    fn has_text(&self) -> bool {
        self.label.as_deref().is_some_and(|l| !l.is_empty())
            || self.content.is_some()
            || !self.children.is_empty()
    }

    /// A close/confirm button with no content draws its ✕ / ✓.
    fn empty_role(&self) -> bool {
        !self.has_text()
            && self.icon.is_none()
            && matches!(self.role, Some(Role::Close | Role::Confirm))
    }

    /// Width = height: icon-only (an icon and no label), the legacy icon size, an empty role.
    fn square(&self) -> bool {
        let icon_only = !self.has_text()
            && (self.icon.is_some() || self.empty_role())
            && self.icon_position != IconPosition::Top;
        icon_only || self.size == ButtonSize::Icon || self.shape == ButtonShape::Circle
    }

    fn circle(&self) -> bool {
        self.square() && self.shape != ButtonShape::Rounded
    }

    fn tile(&self) -> bool {
        self.icon_position == IconPosition::Top && !self.circle()
    }

    fn radius(&self) -> FxPx {
        let control = self.size.control();
        if self.shape == ButtonShape::Rounded {
            control.field_radius().px()
        } else if self.tile() {
            RadiusToken::Lg.px()
        } else {
            RadiusToken::Full.px()
        }
    }

    /// The resolved container [`Style`] (fill, edge, shadow, radius, focus ring, disabled dim).
    /// The same for `Default`, `Hover` and `Pressed`: the button never repaints on hover/press.
    pub fn style(&self, tokens: &dyn Tokens) -> Style {
        let look = self.look(tokens);
        let mut s = Style::new().rounded(self.radius());
        if let Some(bg) = look.background {
            s = s.background(bg);
        }
        if let Some(edge) = look.edge {
            s = s.border_token(tokens, edge);
        }
        if let Some(shadow) = look.shadow {
            s = s.shadow(shadow);
        }
        if look.edge_highlight {
            s = s.shadow_token(tokens, ShadowToken::EdgeHighlight);
        }
        if self.state.shows_focus_ring() {
            s = s.focus_ring(tokens, ColorToken::Accent);
        }
        if self.state.is_disabled() {
            s = s.opacity(self.state.opacity_as(Dim::Action));
        }
        s
    }

    /// The label node in the size's control label (a tile sets it one style smaller).
    fn label_node(&self, text: String, color: Rgba8) -> LayoutNode {
        let control = self.size.control();
        let style = if self.tile() {
            TextStyle {
                white_space: WhiteSpace::NoWrap,
                ..tile_label(control).text_style(color, Some(FontWeight::Medium))
            }
        } else {
            label_style(control, color)
        };
        // An empty label keeps its place in the tree (the DSL addresses the children after
        // it) but leaves the flow, so it adds no gap.
        let position = if text.is_empty() { Position::Absolute } else { Position::Relative };
        LayoutNode::Text(
            TextNode {
                content: TextContent::new(text),
                style,
                max_lines: Some(1),
                item: FlexItem { position, ..FlexItem::default() },
                ..TextNode::default()
            },
            VisualStyle::default(),
        )
    }

    fn role_glyph(&self, color: Rgba8) -> Option<LayoutNode> {
        if !self.empty_role() {
            return None;
        }
        let icon = self.icon_size().0;
        Some(match self.role {
            Some(Role::Close) => {
                glyph::stroke(&GLYPH_CLOSE, false, 26, FxPx::new((icon * 85 + 50) / 100), color)
            }
            _ => glyph::stroke(&GLYPH_CHECK, false, 26, FxPx::new(icon), color),
        })
    }

    /// The content row: [leading icon] label [children…] [trailing icon].
    fn content_node(mut self, tokens: &dyn Tokens) -> LayoutNode {
        let color = self.foreground(tokens);
        let tile = self.tile();
        let gap = if tile { FxPx::new(4) } else { metrics(self.size.control()).gap };
        let icon = self.icon.take().or_else(|| self.role_glyph(color));
        let label = self.label.take().map(|text| self.label_node(text, color));
        let trailing = self.icon_position == IconPosition::Trailing;
        let mut items = Vec::with_capacity(self.children.len() + 2);
        if !trailing {
            items.extend(icon.clone());
        }
        items.extend(label);
        items.append(&mut self.children);
        if trailing {
            items.extend(icon);
        }
        LayoutNode::Stack(
            Stack {
                direction: if tile { Direction::Column } else { Direction::Row },
                gap,
                align: Align::Center,
                justify: Justify::Center,
                ..Stack::default()
            },
            VisualStyle::default(),
            items,
        )
    }

    /// Build the styled container node (centres its content).
    pub fn build(mut self, tokens: &dyn Tokens) -> LayoutNode {
        let style = self.style(tokens);
        let m = metrics(self.size.control());
        let h = m.height;
        let (square, tile, full_width, id) = (self.square(), self.tile(), self.full_width, self.id);
        let content = match self.content.take() {
            Some(content) => content,
            None => self.content_node(tokens),
        };
        let mut button = Button::new().style(style).content(content);
        if let Some(id) = id {
            button = button.id(id);
        }
        let mut node = button.build();
        if let LayoutNode::Stack(stack, _, _) = &mut node {
            if square {
                stack.padding = EdgeInsets::zero();
                stack.min_width = Some(h);
                stack.max_width = Some(h);
                stack.min_height = Some(h);
                stack.max_height = Some(h);
            } else if tile {
                stack.padding = EdgeInsets::symmetric(FxPx::new(8), m.pad_x);
                stack.min_height = Some(FxPx::new((h.0 * 16 + 5) / 10));
            } else {
                stack.padding = EdgeInsets::symmetric(FxPx::ZERO, m.pad_x);
                stack.min_height = Some(h);
                stack.max_height = Some(h);
            }
            if full_width {
                // Across a column (the form case); a flex grow would also stretch the height
                // there, so a row that wants the button to fill says so itself (`.grow`).
                stack.item.align_self = Some(Align::Stretch);
            }
        }
        node
    }
}

/// One text style below the size's control label (the tile layout).
const fn tile_label(size: ControlSize) -> TextStyleToken {
    match size {
        ControlSize::Xs => TextStyleToken::Caption2,
        ControlSize::Sm => TextStyleToken::Caption,
        ControlSize::Md => TextStyleToken::Footnote,
        ControlSize::Lg => TextStyleToken::Subheadline,
        ControlSize::Xl => TextStyleToken::Body,
    }
}

#[cfg(test)]
#[path = "glass_button_tests.rs"]
mod tests;
