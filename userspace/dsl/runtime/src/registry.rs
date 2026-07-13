// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Token vocabulary mapping + widget builders (kind symbol → `LayoutNode`).
//!
//! One deliberate seam: everything the emitter knows about *rendering* a
//! widget kind lives here, so promoting a kind to a richer kit component (or
//! adding one) never touches the walker.

use crate::store::Value;
use alloc::string::String;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, GlassLevel, Justify, LayoutNode,
    Overflow, Spacer, Stack, SurfaceMaterial, TextContent, TextNode, TextStyle, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, MaterialToken, Tokens, TypographyToken};

/// Spacing scale: one step = 4px (matches the theme spacing scale).
#[must_use]
pub fn spacing(step: i64) -> FxPx {
    FxPx::new((step.clamp(0, 256) * 4) as i32)
}

#[must_use]
pub fn color_token(name: &str) -> Option<ColorToken> {
    Some(match name {
        "surface" => ColorToken::Surface,
        "surfaceVariant" => ColorToken::SurfaceVariant,
        "onSurface" => ColorToken::OnSurface,
        "onSurfaceVariant" => ColorToken::OnSurfaceVariant,
        "accent" => ColorToken::Accent,
        "onAccent" => ColorToken::OnAccent,
        "border" => ColorToken::Border,
        "background" => ColorToken::Background,
        "islandBg" => ColorToken::IslandBg,
        "primary" => ColorToken::Primary,
        "onPrimary" => ColorToken::OnPrimary,
        "danger" => ColorToken::Danger,
        _ => return None,
    })
}

#[must_use]
pub fn type_size(name: &str) -> Option<TypographyToken> {
    Some(match name {
        "xs" => TypographyToken::Xs,
        "sm" => TypographyToken::Sm,
        "base" => TypographyToken::Base,
        "md" => TypographyToken::Md,
        "lg" => TypographyToken::Lg,
        "xl" => TypographyToken::Xl,
        _ => return None,
    })
}

#[must_use]
pub fn radius(name: &str) -> FxPx {
    FxPx::new(match name {
        "sm" => 4,
        "md" => 8,
        "lg" => 12,
        "xl" => 16,
        "full" => 9999,
        _ => 0,
    })
}

/// Layout/paint configuration accumulated from a node's modifiers.
pub struct Mods {
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
    pub opacity: Option<u8>,
    pub disabled: bool,
    /// Compositing material (`.material(panel|card|subtle|window|opaque)`) — a
    /// glass node becomes a backdrop-blurred layer at the compositor.
    pub material: Option<SurfaceMaterial>,
    /// `.scroll(vertical|horizontal)`: this container is the page's scroll
    /// viewport — the layout clips it (`Overflow::Hidden`) and the HOST owns
    /// a paint-time scroll offset over the retained boxes (never a re-layout).
    pub scroll: Option<ScrollAxis>,
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
            padding: EdgeInsets::zero(),
            gap: FxPx::ZERO,
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
            opacity: None,
            disabled: false,
            material: None,
            scroll: None,
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
        if let Some(rounded) = self.rounded {
            visual.corner_radius = CornerRadius::uniform(rounded);
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
            // Glass without an explicit `.bg()` paints the design-system
            // material TINT (tokens.glass) — the compositor blurs the
            // backdrop behind the region; tint + blur = the liquid-glass
            // look, one token SSOT (no ad-hoc rgba in pages).
            if visual.background.is_none() {
                if let nexus_layout_types::SurfaceMaterial::Glass(level) = material {
                    use nexus_layout_types::GlassLevel;
                    let token = match level {
                        GlassLevel::Panel => MaterialToken::Panel,
                        GlassLevel::Card => MaterialToken::Card,
                        GlassLevel::Subtle => MaterialToken::Subtle,
                        GlassLevel::Window => MaterialToken::Window,
                    };
                    visual.background = Some(tokens.glass(token).tint);
                }
            }
        }
        visual
    }
}

fn plain_stack(mods: &Mods, tokens: &dyn Tokens, children: alloc::vec::Vec<LayoutNode>) -> LayoutNode {
    // `.width(px)`/`.height(px)` pin the box (min == max); explicit min/max
    // win over the pin so `.width(320).maxWidth(400)` still means something.
    let min_w = mods.min_width.or(mods.width);
    let max_w = mods.max_width.or(mods.width);
    let min_h = mods.min_height.or(mods.height);
    let max_h = mods.max_height.or(mods.height);
    let mut item = FlexItem { flex_grow: mods.grow, ..FlexItem::default() };
    if let Some(shrink) = mods.shrink {
        item.flex_shrink = shrink;
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
    let prop = |name: &str| props.iter().find(|(n, _)| n == name).map(|(_, v)| v);
    match kind {
        "Stack" | "List" => plain_stack(mods, tokens, children),
        "Card" => {
            // Kit promotion: GlassCard (Panel + material tokens) is the SSOT.
            let mut card = nexus_widget_card::GlassCard::new()
                .padding(if mods.padding == EdgeInsets::zero() {
                    spacing(3)
                } else {
                    mods.padding.top
                });
            if mods.direction == Some(Direction::Row) {
                card = card.row();
            }
            for child in children {
                card = card.child(child);
            }
            card.build(tokens)
        }
        "Spacer" => LayoutNode::Spacer(Spacer::default()),
        // ── Design-system kit exposure (TASK-0073/0074): every arm calls the
        //    `userspace/ui/widgets/*` builder — the DSL widget IS the kit
        //    widget (one SSOT; visuals come from tokens, never ad-hoc). ──
        "Badge" => {
            use nexus_widget_badge::{Badge, BadgeVariant};
            let label = prop("label").map(value_text).unwrap_or_default();
            let variant = match prop("variant").map(value_text).as_deref() {
                Some("secondary") => BadgeVariant::Secondary,
                Some("glass") => BadgeVariant::Glass,
                Some("destructive") => BadgeVariant::Destructive,
                Some("success") => BadgeVariant::Success,
                Some("warning") => BadgeVariant::Warning,
                Some("outline") => BadgeVariant::Outline,
                Some("active") => BadgeVariant::Active,
                _ => BadgeVariant::Default,
            };
            let badge = Badge::new().variant(variant);
            let fg = badge.foreground(tokens);
            let label_node = {
                let mut mods = Mods::default();
                mods.text_size = Some(nexus_theme_tokens::TypographyToken::Sm);
                let mut node = text_node(label, &mods, tokens);
                if let LayoutNode::Text(text, _) = &mut node {
                    text.style.color = fg;
                }
                node
            };
            badge.content(label_node).build(tokens)
        }
        "Chip" => {
            use nexus_widget_chip::Chip;
            let label = prop("label").map(value_text).unwrap_or_default();
            let selected = matches!(prop("selected"), Some(Value::Bool(true)));
            let mut chip = Chip::new(label).selected(selected);
            if mods.disabled {
                chip = chip.state(nexus_style::InteractionState::Disabled);
            }
            chip.build(tokens)
        }
        "Avatar" => {
            use nexus_widget_avatar::Avatar;
            let mut avatar = Avatar::new();
            if let Some(initials) = prop("initials").map(value_text) {
                avatar = avatar.initials(initials);
            }
            if let Some(Value::Int(size)) = prop("size") {
                avatar = avatar.size(*size as i32);
            }
            avatar.build(tokens)
        }
        "Checkbox" => {
            use nexus_widget_checkbox::GlassCheckbox;
            let checked = matches!(prop("checked"), Some(Value::Bool(true)));
            let mut cb = GlassCheckbox::new().checked(checked);
            if mods.disabled {
                cb = cb.state(nexus_style::InteractionState::Disabled);
            }
            cb.build(tokens)
        }
        "Slider" => {
            use nexus_widget_slider::Slider;
            let value = match prop("value") {
                Some(Value::Int(v)) => (*v).clamp(0, 100) as u8,
                _ => 0,
            };
            let mut slider = Slider::new().value(value);
            if mods.disabled {
                slider = slider.state(nexus_style::InteractionState::Disabled);
            }
            slider.build(tokens)
        }
        "Spinner" => {
            use nexus_widget_spinner::Spinner;
            // Flat spokes: the host's carousel loop paints the rotating fade
            // as a per-spoke opacity wash (a wash OVER the baked resting fade
            // would double-fade the tail).
            let mut spinner = Spinner::new().flat();
            if let Some(Value::Int(size)) = prop("size") {
                spinner = spinner.size(*size as i32);
            }
            if let Some(fg) = mods.fg {
                spinner = spinner.color(fg);
            }
            spinner.build(tokens)
        }
        "ProgressBar" => {
            use nexus_widget_progress_bar::ProgressBar;
            let mut bar = ProgressBar::new();
            match prop("value") {
                Some(Value::Int(v)) => bar = bar.value((*v).clamp(0, 100) as u32),
                _ => bar = bar.indeterminate(),
            }
            if let Some(Value::Int(h)) = prop("height") {
                bar = bar.height(*h as i32);
            }
            bar.build(tokens)
        }
        "Toast" => {
            use nexus_widget_toast::{Toast, ToastVariant};
            let message = prop("message").map(value_text).unwrap_or_default();
            let mut toast = Toast::new(message);
            toast = toast.variant(match prop("variant").map(value_text).as_deref() {
                Some("success") => ToastVariant::Success,
                Some("warning") => ToastVariant::Warning,
                Some("destructive") => ToastVariant::Destructive,
                _ => ToastVariant::Default,
            });
            if let Some(action) = prop("action").map(value_text).filter(|a| !a.is_empty()) {
                toast = toast.action(action);
            }
            toast.build(tokens)
        }
        "Banner" => {
            use nexus_widget_banner::{Banner, BannerVariant};
            let mut banner = Banner::new();
            if let Some(title) = prop("title").map(value_text).filter(|t| !t.is_empty()) {
                banner = banner.title(title);
            }
            if let Some(message) = prop("message").map(value_text).filter(|m| !m.is_empty()) {
                banner = banner.message(message);
            }
            banner = banner.variant(match prop("variant").map(value_text).as_deref() {
                Some("success") => BannerVariant::Success,
                Some("warning") => BannerVariant::Warning,
                Some("destructive") => BannerVariant::Destructive,
                _ => BannerVariant::Info,
            });
            if let Some(action) = prop("action").map(value_text).filter(|a| !a.is_empty()) {
                banner = banner.action(action);
            }
            banner.build(tokens)
        }
        "Skeleton" => {
            use nexus_widget_skeleton::Skeleton;
            let mut sk = Skeleton::new();
            if let Some(Value::Int(w)) = prop("width") {
                sk = sk.width(*w as i32);
            }
            if let Some(Value::Int(h)) = prop("height") {
                sk = sk.height(*h as i32);
            }
            if matches!(prop("circle"), Some(Value::Bool(true))) {
                sk = sk.circle();
            }
            sk.build(tokens)
        }
        "ListItem" => {
            // Kit promotion: the design-system ListItem (settings rows,
            // search results) — leading/trailing stay DSL children follow-ups;
            // title/subtitle/chevron/destructive map 1:1.
            use nexus_widget_list_item::ListItem;
            let title = prop("title").map(value_text).unwrap_or_default();
            let mut li = ListItem::new(title);
            if let Some(sub) = prop("subtitle").map(value_text) {
                li = li.subtitle(sub);
            }
            if matches!(prop("showChevron"), Some(Value::Bool(true))) {
                li = li.show_chevron(true);
            }
            if matches!(prop("destructive"), Some(Value::Bool(true))) {
                li = li.destructive(true);
            }
            li.build(tokens)
        }
        "Toolbar" => {
            use nexus_widget_toolbar::Toolbar;
            let mut tb = Toolbar::new();
            if let Some(title) = prop("title").map(value_text) {
                tb = tb.title(title);
            }
            if let Some(sub) = prop("subtitle").map(value_text) {
                tb = tb.subtitle(sub);
            }
            if matches!(prop("centerTitle"), Some(Value::Bool(true))) {
                tb = tb.center_title(true);
            }
            tb.build(tokens)
        }
        "SearchBar" => {
            use nexus_widget_search_bar::SearchBar;
            let mut sb = SearchBar::new();
            if let Some(value) = prop("value").map(value_text) {
                sb = sb.value(value);
            }
            if let Some(ph) = prop("placeholder").map(value_text) {
                sb = sb.placeholder(ph);
            }
            if mods.disabled {
                sb = sb.state(nexus_style::InteractionState::Disabled);
            }
            sb.build(tokens)
        }
        "Text" => {
            let value = prop("value").map(value_text).unwrap_or_default();
            text_node(value, mods, tokens)
        }
        "Button" => {
            // Kit promotion: the design-system GlassButton is the SSOT for
            // button visuals; DSL modifiers select variant/state, the kit
            // decides the look. Structure: root → content stack (index 0) →
            // label text (0) + declared children (1+) — `child_path_prefix`
            // mirrors this for handler/child paths.
            let label = prop("label").map(value_text).unwrap_or_default();
            let variant = if mods.bg == Some(ColorToken::Danger) {
                nexus_widget_button::ButtonVariant::Destructive
            } else if mods.bg == Some(ColorToken::SurfaceVariant) {
                nexus_widget_button::ButtonVariant::Secondary
            } else {
                nexus_widget_button::ButtonVariant::Default
            };
            let state = if mods.disabled {
                nexus_style::InteractionState::Disabled
            } else {
                nexus_style::InteractionState::Default
            };
            let label_mods = Mods {
                fg: Some(mods.fg.unwrap_or(ColorToken::OnAccent)),
                text_size: mods.text_size,
                ..Mods::default()
            };
            let mut content = alloc::vec![text_node(label, &label_mods, tokens)];
            content.extend(children);
            let content_mods = Mods {
                direction: Some(Direction::Row),
                align: Some(Align::Center),
                gap: mods.gap,
                ..Mods::default()
            };
            nexus_widget_button::GlassButton::new()
                .variant(variant)
                .state(state)
                .content(plain_stack(&content_mods, tokens, content))
                .build(tokens)
        }
        "TextField" => {
            // Kit promotion: GlassTextField (label + field + focus tokens).
            let mut field = nexus_widget_text_field::GlassTextField::new();
            if let Some(label) = prop("label").map(value_text) {
                field = field.label(label);
            }
            if let Some(value) = prop("value").map(value_text).filter(|v| !v.is_empty()) {
                field = field.value(value);
            }
            if let Some(placeholder) = prop("placeholder").map(value_text) {
                field = field.placeholder(placeholder);
            }
            field.build(tokens)
        }
        "Toggle" => {
            let checked = matches!(prop("checked"), Some(Value::Bool(true)));
            let mut toggle = nexus_widget_toggle::GlassToggle::new().checked(checked);
            if mods.disabled {
                toggle = toggle.state(nexus_style::InteractionState::Disabled);
            }
            toggle.build(tokens)
        }
        "Icon" => {
            // Kit promotion: the vector Icon primitive. Symbol names resolve
            // against the THEME-LINKED icon set first (`[icons.symbols]`,
            // SwiftUI-style vocabulary from the maintained vendor repo), then
            // the built-in fallback shapes (legacy camelCase names).
            let name = prop("symbol").map(value_text).unwrap_or_default();
            // Glyph size in px (launcher tiles need ~28; default 16 = inline).
            let size = match prop("size") {
                Some(Value::Int(s)) => (*s).clamp(8, 96) as i32,
                _ => 16,
            };
            if let Some(lucide) = nexus_widget_icon::lucide_symbol_named(&name) {
                return nexus_widget_icon::Icon::lucide(lucide)
                    .size(size)
                    .color(mods.fg.unwrap_or(ColorToken::OnSurfaceVariant))
                    .build(tokens);
            }
            let symbol = match name.as_str() {
                "plus" => Some(nexus_widget_icon::Symbol::Plus),
                "minus" => Some(nexus_widget_icon::Symbol::Minus),
                "close" => Some(nexus_widget_icon::Symbol::Close),
                "star" => Some(nexus_widget_icon::Symbol::Star),
                "chevronRight" => Some(nexus_widget_icon::Symbol::ChevronRight),
                "chevronLeft" => Some(nexus_widget_icon::Symbol::ChevronLeft),
                "chevronDown" => Some(nexus_widget_icon::Symbol::ChevronDown),
                "chevronUp" => Some(nexus_widget_icon::Symbol::ChevronUp),
                _ => None,
            };
            match symbol {
                Some(symbol) => nexus_widget_icon::Icon::new(symbol)
                    .size(size)
                    .color(mods.fg.unwrap_or(ColorToken::OnSurfaceVariant))
                    .build(tokens),
                None => {
                    // Unknown symbol: honest tinted placeholder box.
                    let box_mods = Mods {
                        bg: Some(mods.fg.unwrap_or(ColorToken::OnSurfaceVariant)),
                        rounded: Some(FxPx::new(3)),
                        padding: EdgeInsets::all(FxPx::new(8)),
                        ..Mods::default()
                    };
                    plain_stack(&box_mods, tokens, alloc::vec![])
                }
            }
        }
        _ => plain_stack(mods, tokens, children),
    }
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
