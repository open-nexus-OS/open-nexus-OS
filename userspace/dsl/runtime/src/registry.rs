// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Token vocabulary mapping + widget builders (kind symbol → `LayoutNode`).
//!
//! One deliberate seam: everything the emitter knows about *rendering* a
//! widget kind lives here, so promoting a kind to a richer kit component (or
//! adding one) never touches the walker.

mod tokens;
mod widgets;

pub use tokens::{
    border_width, color_token, font_weight, leading, radius, shadow_level, spacing, text_align,
    text_shadow, type_size, TextShadowStep,
};

use crate::store::Value;
use alloc::string::String;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, GlassLevel, Justify, LayoutNode,
    Overflow, Stack, SurfaceMaterial, TextContent, TextNode, TextStyle, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, MaterialToken, Tokens, TypographyToken};

/// Layout/paint configuration accumulated from a node's modifiers.
#[derive(Clone)]
pub struct Mods {
    /// `.bgGradient(top, bottom)` — vertical linear fill (wins over `bg`).
    pub bg_gradient: Option<([u8; 4], [u8; 4])>,
    /// `.shadow(sm|md|lg|xl|xxl)` — design elevation (BoxShadow scale).
    pub shadow: Option<nexus_layout_types::ShadowLevel>,
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
    pub wrap: bool,
    pub direction: Option<Direction>,
    pub align: Option<Align>,
    pub justify: Option<Justify>,
    pub bg: Option<ColorToken>,
    pub fg: Option<ColorToken>,
    pub rounded: Option<FxPx>,
    pub text_size: Option<TypographyToken>,
    /// `.fontWeight(light|regular|medium|semibold|bold)` — resolved against
    /// the baked ladder at paint time (RFC-0082).
    pub font_weight: Option<nexus_layout_types::FontWeight>,
    /// `.leading(flat|tight|snug|normal|relaxed)` — line height as a
    /// percentage of the font size; `None` = the face's own line height.
    pub leading: Option<nexus_theme_tokens::LeadingToken>,
    /// `.textAlign(left|center|right)`.
    pub text_align: Option<nexus_layout_types::TextAlign>,
    /// `.textShadow(none|soft|strong)` — legibility for text sitting on a
    /// wallpaper (RFC-0082). NOT a blurred shadow; see [`TextShadowStep`].
    pub text_shadow: Option<TextShadowStep>,
    /// `.border(<length token>)` — hairline width; pairs with `border_color`.
    pub border_width: Option<FxPx>,
    /// `.borderColor(<color token>)`.
    pub border_color: Option<ColorToken>,
    /// `.hitSlop(n)` — outward growth of the input rect only (spacing scale,
    /// 1 step = 4px). Layout and pixels are untouched; see
    /// [`nexus_layout_types::FlexItem::hit_slop`].
    pub hit_slop: nexus_layout_types::FxPx,
    pub opacity: Option<u8>,
    pub disabled: bool,
    /// Compositing material (`.material(panel|card|subtle|window|opaque)`) — a
    /// glass node becomes a backdrop-blurred layer at the compositor.
    pub material: Option<SurfaceMaterial>,
    /// `.scroll(vertical|horizontal)`: this container is the page's scroll
    /// viewport — the layout clips it (`Overflow::Hidden`) and the HOST owns
    /// a paint-time scroll offset over the retained boxes (never a re-layout).
    pub scroll: Option<ScrollAxis>,
    /// `.overlay()`: lift this container OUT OF FLOW as a full-bleed layer
    /// over its parent's content (drop-down panels / dialogs). Anchoring
    /// happens INSIDE the layer with ordinary flex.
    pub overlay: bool,
}

/// Scroll axis of a `.scroll(...)` viewport.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

impl Default for Mods {
    fn default() -> Self {
        Self {
            bg_gradient: None,
            shadow: None,
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
            shrink: None,
            wrap: false,
            direction: None,
            align: None,
            justify: None,
            bg: None,
            fg: None,
            rounded: None,
            text_size: None,
            font_weight: None,
            leading: None,
            text_align: None,
            text_shadow: None,
            border_width: None,
            border_color: None,
            opacity: None,
            disabled: false,
            material: None,
            scroll: None,
            overlay: false,
        }
    }
}

/// Maps a `.material(<token>)` name to its [`SurfaceMaterial`]. Unknown tokens
/// return `None` (the checker rejects them; here they leave the default opaque).
pub fn material_token(name: &str) -> Option<SurfaceMaterial> {
    Some(match name {
        "opaque" => SurfaceMaterial::Opaque,
        "panel" => SurfaceMaterial::Glass(GlassLevel::Panel),
        "card" => SurfaceMaterial::Glass(GlassLevel::Card),
        "subtle" => SurfaceMaterial::Glass(GlassLevel::Subtle),
        "window" => SurfaceMaterial::Glass(GlassLevel::Window),
        "overlay" => SurfaceMaterial::Glass(GlassLevel::Overlay),
        _ => return None,
    })
}

impl Mods {
    /// The paint part as a `VisualStyle`.
    pub fn visual(&self, tokens: &dyn Tokens) -> VisualStyle {
        let mut visual = VisualStyle::default();
        if let Some(bg) = self.bg {
            visual.background = Some(tokens.color(bg));
        }
        if let Some((top, bottom)) = self.bg_gradient {
            visual.background_gradient = Some((
                nexus_layout_types::Rgba8 { r: top[0], g: top[1], b: top[2], a: top[3] },
                nexus_layout_types::Rgba8 {
                    r: bottom[0],
                    g: bottom[1],
                    b: bottom[2],
                    a: bottom[3],
                },
            ));
        }
        if let Some(rounded) = self.rounded {
            visual.corner_radius = CornerRadius::uniform(rounded);
        }
        if let Some(level) = self.shadow {
            visual.shadow = Some(level.to_box_shadow());
        }
        if let Some(opacity) = self.opacity {
            visual.opacity = Some(nexus_layout_types::Fraction(u32::from(opacity)));
        }
        if self.disabled {
            // The InteractionState::Disabled wash (140/255).
            visual.opacity = Some(nexus_layout_types::Fraction(140));
        }
        if let Some(material) = self.material {
            visual.material = material;
            // Glass without an explicit `.bg()` takes the design-system
            // recipe: tint, capped shine wash, `inset 0 1px 0` top-shine and
            // the 1px hairline. `Style::glass` is the ONE definition — kit
            // widgets call the same builder, so a page and a hand-written
            // widget cannot render the same level differently (RFC-0082).
            if visual.background.is_none() {
                if let nexus_layout_types::SurfaceMaterial::Glass(level) = material {
                    use nexus_layout_types::GlassLevel;
                    let token = match level {
                        GlassLevel::Panel => MaterialToken::Panel,
                        GlassLevel::Card => MaterialToken::Card,
                        GlassLevel::Subtle => MaterialToken::Subtle,
                        GlassLevel::Window => MaterialToken::Window,
                        GlassLevel::Overlay => MaterialToken::Overlay,
                    };
                    let glass = nexus_style::Style::new().glass(token, tokens).visual();
                    visual.background = glass.background;
                    visual.inset_highlight = glass.inset_highlight;
                    if visual.background_gradient.is_none() {
                        visual.background_gradient = glass.background_gradient;
                    }
                    if visual.border == nexus_layout_types::EdgeBorder::none() {
                        visual.border = glass.border;
                    }
                }
            }
        }
        // An explicit `.border()/.borderColor()` overrides the material's
        // hairline (both default to the theme's value when only one is given).
        if self.border_width.is_some() || self.border_color.is_some() {
            let width = self
                .border_width
                .unwrap_or_else(|| tokens.length(nexus_theme_tokens::LengthToken::BorderThin));
            let color = self
                .border_color
                .map_or_else(|| tokens.color(ColorToken::Border), |token| tokens.color(token));
            visual.border = nexus_layout_types::EdgeBorder::all(width, color);
        }
        if let Some(shadow) = self.text_shadow {
            visual.text_shadow = Some(shadow.resolve(tokens));
        }
        visual
    }
}

fn plain_stack(
    mods: &Mods,
    tokens: &dyn Tokens,
    children: alloc::vec::Vec<LayoutNode>,
) -> LayoutNode {
    // `.width(px)`/`.height(px)` pin the box (min == max); explicit min/max
    // win over the pin so `.width(320).maxWidth(400)` still means something.
    let min_w = mods.min_width.or(mods.width);
    let max_w = mods.max_width.or(mods.width);
    let min_h = mods.min_height.or(mods.height);
    let max_h = mods.max_height.or(mods.height);
    let mut item =
        FlexItem { flex_grow: mods.grow, hit_slop: mods.hit_slop, ..FlexItem::default() };
    if let Some(shrink) = mods.shrink {
        item.flex_shrink = shrink;
    }
    if mods.overlay {
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
            // `.scroll(...)` clips this container (the scroll viewport); the
            // engine then stamps `clip_rect` on every descendant box, which is
            // what the host's paint-time scroll offset keys on.
            overflow: match mods.scroll {
                Some(ScrollAxis::Horizontal) => {
                    Overflow::Scroll(nexus_layout_types::ScrollAxis::Horizontal)
                }
                Some(ScrollAxis::Vertical) => {
                    Overflow::Scroll(nexus_layout_types::ScrollAxis::Vertical)
                }
                None => Overflow::Visible,
            },
            flex_wrap: mods.wrap,
            min_width: min_w,
            max_width: max_w,
            min_height: min_h,
            max_height: max_h,
            item,
        },
        mods.visual(tokens),
        children,
    )
}

fn text_node(value: String, mods: &Mods, tokens: &dyn Tokens) -> LayoutNode {
    let mut style = TextStyle {
        color: tokens.color(mods.fg.unwrap_or(ColorToken::OnSurface)),
        ..TextStyle::default()
    };
    if let Some(size) = mods.text_size {
        style.font_size = tokens.type_size(size);
    }
    if let Some(weight) = mods.font_weight {
        style.font_weight = weight;
    }
    if let Some(align) = mods.text_align {
        style.text_align = align;
    }
    // `.leading()` is the only EXPLICIT line-height signal; without it the
    // struct default stays and the text measurer uses the baked face's own
    // line height (RFC-0082 — `BakedTextMeasure::line_height`).
    if let Some(step) = mods.leading {
        style.line_height =
            nexus_layout_types::LineHeight::Relative(FxPx::new(tokens.leading_pct(step) as i32));
    }
    LayoutNode::Text(
        TextNode {
            id: None,
            content: TextContent(value),
            style,
            item: FlexItem::default(),
            max_lines: None,
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
    if mods.grow > 0 || mods.shrink.is_some() {
        if let Some(item) = node_item_mut(&mut node) {
            if mods.grow > 0 {
                item.flex_grow = mods.grow;
            }
            if let Some(shrink) = mods.shrink {
                item.flex_shrink = shrink;
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
