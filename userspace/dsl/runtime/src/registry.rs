// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Token vocabulary mapping + widget builders (kind symbol → `LayoutNode`).
//!
//! One deliberate seam: everything the emitter knows about *rendering* a
//! widget kind lives here, so promoting a kind to a richer kit component (or
//! adding one) never touches the walker.

mod controls;
mod pane;
mod tokens;
mod widgets;

pub(crate) use pane::PaneTokens;

pub use tokens::{
    border_width, color_token, figures, font_weight, material, radius, shadow, spacing,
    spacing_token, text_align, text_style, tint_alpha, ShadowName,
};

use crate::store::Value;
use alloc::string::String;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Material,
    Overflow, Rgba8, Stack, TextContent, TextNode, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, ShadowToken, TextStyleToken, Tokens};

/// Layout/paint configuration accumulated from a node's modifiers.
#[derive(Clone)]
pub struct Mods {
    /// `.bgGradient(top, bottom)` — vertical linear fill (wins over `bg`).
    pub bg_gradient: Option<([u8; 4], [u8; 4])>,
    /// `.shadow(a, b?)` — an elevation (`shadow1`…`shadow4`), the recessed well (`shadowInset`),
    /// the lit top edge (`edgeHighlight`) and/or the label halo (`halo`), resolved against the
    /// theme (RFC-0116).
    pub shadows: [Option<ShadowName>; 2],
    pub padding: EdgeInsets,
    pub gap: FxPx,
    /// Fixed box sizes in raw px (`.width(320)`); `full` is a no-op today
    /// (cross-axis children already stretch by default).
    pub width: Option<FxPx>,
    pub height: Option<FxPx>,
    pub min_width: Option<FxPx>,
    pub max_width: Option<FxPx>,
    pub min_height: Option<FxPx>,
    pub max_height: Option<FxPx>,
    pub grow: u32,
    pub shrink: Option<u32>,
    /// `.basis(n)` — flex base size on the parent's main axis.
    pub basis: Option<FxPx>,
    /// `.textFit(pct, min, max)` — box-relative type for this container's
    /// text descendants.
    pub text_fit: Option<nexus_layout_types::TextFit>,
    pub wrap: bool,
    pub direction: Option<Direction>,
    pub align: Option<Align>,
    pub justify: Option<Justify>,
    pub bg: Option<ColorToken>,
    pub fg: Option<ColorToken>,
    /// `.tint(role, level?)` — the role at a wash strength (alpha), as the background.
    pub tint: Option<(ColorToken, u8)>,
    pub rounded: Option<FxPx>,
    /// `.corners(tl, tr, br, bl)` — per-corner radii (wins over `rounded`).
    pub corners: Option<CornerRadius>,
    /// `.textStyle(<style>)` — size, line height, weight and tracking together (RFC-0116);
    /// `None` = `body`.
    pub text_style: Option<TextStyleToken>,
    /// `.fontWeight(light|normal|medium|semibold|bold)` — the one override of the style's weight.
    pub font_weight: Option<nexus_layout_types::FontWeight>,
    /// `.figures(tabular|proportional)` — overrides the style's figure width (`Some(true)` =
    /// tabular digits).
    pub figures: Option<bool>,
    /// `.textAlign(left|center|right)`.
    pub text_align: Option<nexus_layout_types::TextAlign>,
    /// `.truncate(n)`: the text keeps at most `n` lines (the painter cuts the last with an
    /// ellipsis); `None` = it wraps freely.
    pub max_lines: Option<u32>,
    /// `.border(hairline|ring)` — width; pairs with `border_color`.
    pub border_width: Option<FxPx>,
    /// `.borderColor(<color token>)`.
    pub border_color: Option<ColorToken>,
    /// `.borderTint(role)` — a border in the role at the `border` wash strength (30 %).
    pub border_tint: Option<ColorToken>,
    /// `.hitSlop(px)` — outward growth of the input rect only. Layout and pixels are untouched;
    /// see [`nexus_layout_types::FlexItem::hit_slop`].
    pub hit_slop: nexus_layout_types::FxPx,
    pub opacity: Option<u8>,
    pub disabled: bool,
    /// Compositing material (`.material(panel|window|dock|thin|ultraThin|card|pane|overlay)`,
    /// RFC-0116) — a blur material becomes a backdrop-blurred region at the compositor, an
    /// opaque one an occluder; the recipe (fill, edge, shadow) paints here.
    pub material: Option<Material>,
    /// `.scroll(vertical|horizontal|paged)`: this container is a scroll layer (RFC-0109) — its
    /// content is drawn once into a band and the compositor moves it by the offset the host's
    /// physics commits (never a re-layout, never a re-raster per step). Several per page.
    pub scroll: Option<ScrollAxis>,
    /// `.overflow(hidden)`: this container clips its content (`Overflow::Hidden`) — no scroll.
    pub clip: bool,
    /// `.overlay(kind?)`: lift this container OUT OF FLOW as a full-bleed
    /// layer over its parent's content (drop-down panels / dialogs). Anchoring
    /// happens INSIDE the layer with ordinary flex. The kind (TASK-0074) adds
    /// modal/transient semantics on top; `None` = in flow.
    pub overlay: Option<crate::overlay::OverlayKind>,
    /// `.dismissAfter(ms)` of a transient overlay (bounded at emit).
    pub dismiss_after: Option<u32>,
    /// `.autofocus(true)`: the host focuses this text field whenever none holds focus.
    pub autofocus: bool,
    /// `.columns(n)`: this container is an n-column GRID (`LayoutNode::Grid`,
    /// row-major 1fr tracks). On a `List` the items are the cells — the
    /// data-driven launcher/workspace grids.
    pub columns: Option<usize>,
    /// `.rowGap(n)` — the grid's row gap (spacing scale); `None` = `.gap()`.
    pub row_gap: Option<FxPx>,
}

/// A container's overflow from its modifiers: `.scroll(...)` → a scroll root of its axis,
/// `.overflow(hidden)` → a clip, otherwise visible.
fn overflow_of(mods: &Mods) -> Overflow {
    use nexus_layout_types::ScrollAxis as Axis;
    match (mods.scroll, mods.clip) {
        (Some(ScrollAxis::Horizontal), _) => Overflow::Scroll(Axis::Horizontal),
        (Some(ScrollAxis::Paged), _) => Overflow::Scroll(Axis::Paged),
        (Some(ScrollAxis::Vertical), _) => Overflow::Scroll(Axis::Vertical),
        (None, true) => Overflow::Hidden,
        (None, false) => Overflow::Visible,
    }
}

/// Scroll axis of a `.scroll(...)` viewport (`paged` = horizontal with
/// host-side page snap — the launcher pager).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
    Paged,
}

impl Default for Mods {
    fn default() -> Self {
        Self {
            bg_gradient: None,
            shadows: [None, None],
            padding: EdgeInsets::zero(),
            gap: FxPx::ZERO,
            hit_slop: FxPx::ZERO,
            width: None,
            height: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            grow: 0,
            basis: None,
            text_fit: None,
            shrink: None,
            wrap: false,
            direction: None,
            align: None,
            justify: None,
            bg: None,
            fg: None,
            tint: None,
            rounded: None,
            corners: None,
            text_style: None,
            font_weight: None,
            figures: None,
            text_align: None,
            max_lines: None,
            border_width: None,
            border_color: None,
            border_tint: None,
            opacity: None,
            disabled: false,
            material: None,
            scroll: None,
            clip: false,
            overlay: None,
            dismiss_after: None,
            autofocus: false,
            columns: None,
            row_gap: None,
        }
    }
}

/// `color` at `alpha` (a role's own alpha multiplied — `accentGlow` is already translucent).
fn with_alpha(color: Rgba8, alpha: u8) -> Rgba8 {
    let a = (u16::from(color.a) * u16::from(alpha) + 127) / 255;
    Rgba8 { a: a as u8, ..color }
}

impl Mods {
    /// The paint part as a `VisualStyle`.
    pub fn visual(&self, tokens: &dyn Tokens) -> VisualStyle {
        let mut visual = VisualStyle::default();
        if let Some(bg) = self.bg {
            visual.background = Some(tokens.color(bg));
        }
        if let Some((role, alpha)) = self.tint {
            visual.background = Some(with_alpha(tokens.color(role), alpha));
        }
        if let Some((top, bottom)) = self.bg_gradient {
            visual.background_gradient = Some((
                Rgba8 { r: top[0], g: top[1], b: top[2], a: top[3] },
                Rgba8 { r: bottom[0], g: bottom[1], b: bottom[2], a: bottom[3] },
            ));
        }
        if let Some(rounded) = self.rounded {
            visual.corner_radius = CornerRadius::uniform(rounded);
        }
        if let Some(corners) = self.corners {
            visual.corner_radius = corners;
        }
        for shadow in self.shadows.into_iter().flatten() {
            let token = match shadow {
                // The halo: the design's soft glow behind the glyphs, in `labelShadow`.
                ShadowName::Halo => {
                    visual.text_shadows =
                        nexus_layout_types::TextShadow::halo(tokens.color(ColorToken::LabelShadow))
                            .map(Some);
                    continue;
                }
                ShadowName::Token(token) => token,
            };
            let spec = tokens.shadow(token);
            match token {
                // Both draw inside the box as the one-pixel top line the painter has: the lit
                // edge in its light colour, the well (`shadowInset`) in its dark one.
                ShadowToken::EdgeHighlight | ShadowToken::Inset => {
                    visual.inset_highlight = Some(spec.color);
                }
                _ => visual.shadow = Some(nexus_style::box_shadow(spec)),
            }
        }
        if let Some(opacity) = self.opacity {
            visual.opacity = Some(nexus_layout_types::Fraction(u32::from(opacity)));
        }
        if self.disabled {
            // A disabled container dims like an action (the design's default family, 0.50);
            // the kit controls dim by their own family (`nexus_style::Dim`).
            visual.opacity = Some(nexus_style::Dim::default().opacity());
        }
        if let Some(material) = self.material {
            // The material's recipe (RFC-0116: fill, glass edge, shadow, edge highlight) is the
            // ONE definition — `Style::material`, which every kit widget calls too, so a page
            // and a hand-written widget cannot render the same material differently. What the
            // page set explicitly wins; the compositor reads `visual.material` for the blur.
            let recipe = nexus_style::Style::new().material(material, tokens).visual();
            visual.material = Some(material);
            if visual.background.is_none() && visual.background_gradient.is_none() {
                visual.background = recipe.background;
            }
            if visual.border == nexus_layout_types::EdgeBorder::none() {
                visual.border = recipe.border;
            }
            if visual.shadow.is_none() {
                visual.shadow = recipe.shadow;
            }
            if visual.inset_highlight.is_none() {
                visual.inset_highlight = recipe.inset_highlight;
            }
        }
        // An explicit `.border()/.borderColor()/.borderTint()` overrides the material's edge
        // (the width defaults to the hairline, the colour to `separator`).
        if self.border_width.is_some() || self.border_color.is_some() || self.border_tint.is_some()
        {
            let width = self.border_width.unwrap_or(nexus_theme_tokens::HAIRLINE);
            let color = match (self.border_tint, self.border_color) {
                (Some(role), _) => with_alpha(tokens.color(role), tint_alpha("border")),
                (None, Some(role)) => tokens.color(role),
                (None, None) => tokens.color(ColorToken::Separator),
            };
            visual.border = nexus_layout_types::EdgeBorder::all(width, color);
        }
        visual
    }
}

fn plain_stack(
    mods: &Mods,
    tokens: &dyn Tokens,
    children: alloc::vec::Vec<LayoutNode>,
) -> LayoutNode {
    // `.columns(n)` re-shapes ANY plain container into the n-column grid —
    // on a `List` the spliced items become the cells (the data-driven
    // launcher/workspace grids). One lowering; the `Grid` widget is sugar.
    if let Some(cols) = mods.columns {
        return plain_grid(cols, mods.row_gap, mods, tokens, children);
    }
    // `.width(px)`/`.height(px)` pin the box (min == max); explicit min/max
    // win over the pin so `.width(320).maxWidth(400)` still means something.
    let min_w = mods.min_width.or(mods.width);
    let max_w = mods.max_width.or(mods.width);
    let min_h = mods.min_height.or(mods.height);
    let max_h = mods.max_height.or(mods.height);
    let mut item = FlexItem {
        flex_grow: mods.grow,
        hit_slop: mods.hit_slop,
        flex_basis: mods.basis,
        ..FlexItem::default()
    };
    if let Some(shrink) = mods.shrink {
        item.flex_shrink = shrink;
    }
    if mods.overlay.is_some() {
        // Out-of-flow layer: absolute + grow = the engine's overlay contract
        // (the layer FILLS the parent's content box — definite constraints,
        // the viewport-root fill semantic). Anchoring happens inside.
        item.position = nexus_layout_types::Position::Absolute;
        item.flex_grow = item.flex_grow.max(1);
    }
    LayoutNode::Stack(
        Stack {
            id: None,
            direction: mods.direction.unwrap_or(Direction::Column),
            gap: mods.gap,
            padding: mods.padding,
            align: mods.align.unwrap_or(Align::Stretch),
            justify: mods.justify.unwrap_or(Justify::Start),
            // `.scroll(...)` makes this container a scroll root (RFC-0109); `.overflow(hidden)`
            // clips it.
            overflow: overflow_of(mods),
            flex_wrap: mods.wrap,
            min_width: min_w,
            max_width: max_w,
            min_height: min_h,
            max_height: max_h,
            item,
            text_fit: mods.text_fit,
        },
        mods.visual(tokens),
        children,
    )
}

/// The `Grid` widget's lowering: the engine's `LayoutNode::Grid` (row-major,
/// n equal `1fr` tracks). Shares the `plain_stack` mod plumbing — width/height
/// pins, grow/shrink, `.overlay()`, `.scroll()` — so a Grid composes exactly
/// like a Stack; only `direction/align/justify/wrap` don't apply (a grid has
/// no main axis).
fn plain_grid(
    columns: usize,
    row_gap: Option<FxPx>,
    mods: &Mods,
    tokens: &dyn Tokens,
    children: alloc::vec::Vec<LayoutNode>,
) -> LayoutNode {
    let min_w = mods.min_width.or(mods.width);
    let max_w = mods.max_width.or(mods.width);
    let min_h = mods.min_height.or(mods.height);
    let max_h = mods.max_height.or(mods.height);
    let mut item = FlexItem {
        flex_grow: mods.grow,
        hit_slop: mods.hit_slop,
        flex_basis: mods.basis,
        ..FlexItem::default()
    };
    if let Some(shrink) = mods.shrink {
        item.flex_shrink = shrink;
    }
    if mods.overlay.is_some() {
        item.position = nexus_layout_types::Position::Absolute;
        item.flex_grow = item.flex_grow.max(1);
    }
    LayoutNode::Grid(
        nexus_layout_types::Grid {
            id: None,
            columns: alloc::vec![nexus_layout_types::Fraction(1); columns.clamp(1, 12)],
            gap: mods.gap,
            row_gap,
            padding: mods.padding,
            overflow: overflow_of(mods),
            min_width: min_w,
            max_width: max_w,
            min_height: min_h,
            max_height: max_h,
            item,
            text_fit: mods.text_fit,
        },
        mods.visual(tokens),
        children,
    )
}

fn text_node(value: String, mods: &Mods, tokens: &dyn Tokens) -> LayoutNode {
    // RFC-0116: a text node is one of the thirteen styles (`body` unless it says otherwise) — size,
    // line box, weight and tracking together; `.fontWeight` is the one override and `overline`
    // sets its text in upper case.
    let style_token = mods.text_style.unwrap_or(TextStyleToken::Body);
    let color = tokens.color(mods.fg.unwrap_or(ColorToken::Label));
    let mut style = style_token.text_style(color, mods.font_weight);
    if let Some(align) = mods.text_align {
        style.text_align = align;
    }
    if let Some(tabular) = mods.figures {
        style.tabular = tabular;
    }
    let value = if style_token.spec().upper { value.to_uppercase() } else { value };
    LayoutNode::Text(
        TextNode {
            id: None,
            content: TextContent(value),
            style,
            item: FlexItem::default(),
            max_lines: mods.max_lines,
            min_width: None,
            max_width: None,
        },
        mods.visual(tokens),
    )
}

/// A numeric prop as raw pixels (`Circle { size: 44 }`); `None` for
/// non-numeric values (the checker flags them, this stays fail-soft).
fn value_px(value: &Value) -> Option<FxPx> {
    match value {
        Value::Int(i) => Some(FxPx::new(*i as i32)),
        Value::Fx(raw) => Some(FxPx::new((raw >> 32) as i32)),
        _ => None,
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::Str(s) => s.clone(),
        Value::Int(i) => alloc::format!("{i}"),
        Value::Bool(b) => String::from(if *b { "true" } else { "false" }),
        Value::Fx(raw) => alloc::format!("{}", raw >> 32),
        _ => String::new(),
    }
}

/// Builds one widget kind. `primary` = the `value`-like prop (resolved),
/// `props` = (name symbol text, value), `children` already emitted.
pub fn build_widget(
    kind: &str,
    props: &[(String, Value)],
    mods: &Mods,
    tokens: &dyn Tokens,
    children: alloc::vec::Vec<LayoutNode>,
) -> LayoutNode {
    let mut node = widgets::build_widget_inner(kind, props, mods, tokens, children);
    // FLEX participation applies to EVERY kind. `plain_stack` sets it from
    // `mods` itself; the kit arms build their own node and would otherwise
    // drop `.grow()`/`.shrink()` on the floor — which is how a `.grow(1)`
    // TextField ended up 0px wide inside a row.
    if mods.grow > 0 || mods.shrink.is_some() || mods.basis.is_some() {
        if let Some(item) = node_item_mut(&mut node) {
            if mods.grow > 0 {
                item.flex_grow = mods.grow;
            }
            if let Some(shrink) = mods.shrink {
                item.flex_shrink = shrink;
            }
            // Without this a `.basis(0)` Button keeps its label width and
            // the row stops dividing evenly — the same class of bug as the
            // 0px-wide `.grow(1)` TextField above.
            if mods.basis.is_some() {
                item.flex_basis = mods.basis;
            }
        }
    }
    node
}

/// The mutable `FlexItem` of any node kind.
fn node_item_mut(node: &mut LayoutNode) -> Option<&mut FlexItem> {
    Some(match node {
        LayoutNode::Stack(stack, _, _) => &mut stack.item,
        LayoutNode::Grid(grid, _, _) => &mut grid.item,
        LayoutNode::Text(text, _) => &mut text.item,
        LayoutNode::TextInput(input, _) => &mut input.item,
        LayoutNode::Spacer(spacer) => &mut spacer.item,
    })
}

/// Where a kind's *declared* children live inside the produced tree:
/// (path prefix from the widget root, index of the first declared child).
/// Mirrors the kit builders' structure — update together with `build_widget`.
#[must_use]
pub fn child_path(kind: &str) -> (&'static [u32], u32) {
    match kind {
        // GlassButton: root → content stack (0) → label (0), children (1+).
        "Button" => (&[0], 1),
        _ => (&[], 0),
    }
}

/// Pre-order box-id offset from a kind's handler node to the part the PRESS
/// interaction animates (see `HandlerEntry::press_offset`). Mirrors the kit
/// builders' structure — update together with `build_widget`.
#[must_use]
pub fn press_offset(kind: &str) -> u32 {
    match kind {
        // GlassToggle: root = the track, sole child (+1 pre-order) = the
        // thumb — the press stretches the thumb along the travel axis.
        "Toggle" => 1,
        _ => 0,
    }
}
