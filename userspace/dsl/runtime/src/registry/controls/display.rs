// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The display and feedback arms of the DSL catalog (`Badge`, `Card`, `Skeleton`, `Spinner`,
//! `ProgressBar`, `Banner`, `Toast`, `Slider`): the props the kit widgets already carry, made
//! reachable from a page.
//!
//! CONTEXT: TASK-0353 P7 (wave B). Every arm calls the SAME `userspace/ui/widgets/*` builder the
//! rest of the OS uses; an unknown value falls back to the widget's default, so a typo renders
//! the default widget instead of nothing. Symbol names (`icon:`) resolve against the icon set
//! here, not in the widget crates, which stay free of the set.

use super::{control_size, flag, prop, text, tint, Props};
use crate::registry::{value_text, Mods};
use crate::store::Value;
use nexus_layout_types::LayoutNode;
use nexus_style::InteractionState;
use nexus_theme_tokens::{ColorToken, Tokens};

/// A stroked icon node from the symbol name in `icon:` (`None` when absent or unknown).
fn icon_node(
    props: Props<'_>,
    size: i32,
    color: ColorToken,
    tokens: &dyn Tokens,
) -> Option<LayoutNode> {
    let symbol = nexus_widget_icon::lucide_symbol_named(&text(props, "icon")?)?;
    Some(nexus_widget_icon::Icon::lucide(symbol).size(size).color(color).build(tokens))
}

/// `Badge { label, variant, tint, size, dot, max }`. The widget sets the label in the size's
/// text style (weight 500; a string of digits is a count: weight 600, tabular, capped by `max`).
/// A `tint` without a `variant` fills in that role.
pub(in crate::registry) fn badge(props: Props<'_>, tokens: &dyn Tokens) -> LayoutNode {
    use nexus_widget_badge::{Badge, BadgeVariant};
    let variant = match text(props, "variant").as_deref() {
        Some("default") => Some(BadgeVariant::Default),
        Some("secondary") => Some(BadgeVariant::Secondary),
        Some("glass") => Some(BadgeVariant::Glass),
        Some("destructive") => Some(BadgeVariant::Destructive),
        Some("success") => Some(BadgeVariant::Success),
        Some("warning") => Some(BadgeVariant::Warning),
        Some("outline") => Some(BadgeVariant::Outline),
        Some("active") => Some(BadgeVariant::Active),
        Some("filled") => Some(BadgeVariant::Filled),
        Some("tinted") => Some(BadgeVariant::Tinted),
        _ => None,
    };
    let mut badge = Badge::new().dot(flag(props, "dot"));
    if let Some(variant) = variant {
        badge = badge.variant(variant);
    }
    if let Some(tint) = tint(props) {
        badge = badge.tint(tint);
    }
    if let Some(size) = control_size(props) {
        badge = badge.size(size);
    }
    if let Some(Value::Int(max)) = prop(props, "max") {
        badge = badge.max((*max).clamp(0, i64::from(u32::MAX)) as u32);
    }
    badge.label(prop(props, "label").map(value_text).unwrap_or_default()).build(tokens)
}

/// `Card { variant, material }` around its children (a column, or a row with
/// `.direction(row)`); the padding is `.padding(n)` (default `gapGroup`).
pub(in crate::registry) fn card(
    props: Props<'_>,
    mods: &Mods,
    tokens: &dyn Tokens,
    children: alloc::vec::Vec<LayoutNode>,
) -> LayoutNode {
    use nexus_layout_types::{Direction, EdgeInsets};
    use nexus_theme_tokens::SpaceToken;
    use nexus_widget_card::{CardLevel, GlassCard};
    let level = match text(props, "variant").as_deref() {
        Some("dock") => CardLevel::Dock,
        Some("card") => CardLevel::Card,
        Some("inner") => CardLevel::Inner,
        Some("subtle") => CardLevel::Subtle,
        Some("overlay") => CardLevel::Overlay,
        _ => CardLevel::Panel,
    };
    let padding = if mods.padding == EdgeInsets::zero() {
        SpaceToken::GapGroup.px()
    } else {
        mods.padding.top
    };
    let mut card = GlassCard::new()
        .level(level)
        .material(!matches!(prop(props, "material"), Some(Value::Bool(false))))
        .padding(padding)
        .children(children);
    if mods.direction == Some(Direction::Row) {
        card = card.row();
    }
    card.build(tokens)
}

/// `Skeleton { width, height, radius, circle }`.
pub(in crate::registry) fn skeleton(props: Props<'_>, tokens: &dyn Tokens) -> LayoutNode {
    use nexus_widget_skeleton::Skeleton;
    let mut sk = Skeleton::new();
    if let Some(Value::Int(w)) = prop(props, "width") {
        sk = sk.width(*w as i32);
    }
    if let Some(Value::Int(h)) = prop(props, "height") {
        sk = sk.height(*h as i32);
    }
    if let Some(Value::Int(r)) = prop(props, "radius") {
        sk = sk.radius(*r as i32);
    }
    if flag(props, "circle") {
        sk = sk.circle();
    }
    sk.build(tokens)
}

/// `Spinner { size, tint }`: `size` is a control size name (`xs`…`xl`: 14 / 20 / 28 / 36 / 44 px)
/// or a diameter in px. Flat spokes: the host's carousel loop paints the rotating fade as a
/// per-spoke opacity wash (a wash OVER the baked resting fade would double-fade the tail).
pub(in crate::registry) fn spinner(
    props: Props<'_>,
    mods: &Mods,
    tokens: &dyn Tokens,
) -> LayoutNode {
    use nexus_widget_spinner::Spinner;
    let mut spinner = Spinner::new().flat();
    if let Some(size) = control_size(props) {
        spinner = spinner.control_size(size);
    } else if let Some(Value::Int(size)) = prop(props, "size") {
        spinner = spinner.size(*size as i32);
    }
    if let Some(color) = tint(props).or(mods.fg) {
        spinner = spinner.color(color);
    }
    spinner.build(tokens)
}

/// `ProgressBar { value, size, tint, label, showValue, appearance, height }`. No `Int` value is
/// the indeterminate pip (the host's sweep loop moves it); `appearance: "circular"` is the ring.
pub(in crate::registry) fn progress_bar(
    props: Props<'_>,
    mods: &Mods,
    tokens: &dyn Tokens,
) -> LayoutNode {
    use nexus_widget_progress_bar::{ProgressAppearance, ProgressBar};
    let mut bar = ProgressBar::new();
    match prop(props, "value") {
        Some(Value::Int(v)) => bar = bar.value((*v).clamp(0, 100) as u32),
        _ => bar = bar.indeterminate(),
    }
    if let Some(Value::Int(h)) = prop(props, "height") {
        bar = bar.height(*h as i32);
    }
    // `.fg()` picks the fill role, the way it does for `Spinner`: a battery meter is not
    // progress-blue, and a page needs a way to say "this bar means charge".
    if let Some(color) = tint(props).or(mods.fg) {
        bar = bar.color(color);
    }
    if let Some(size) = control_size(props) {
        bar = bar.size(size);
    }
    if let Some(label) = text(props, "label") {
        bar = bar.label(label);
    }
    if flag(props, "showValue") {
        bar = bar.show_value(true);
    }
    if text(props, "appearance").as_deref() == Some("circular") {
        bar = bar.appearance(ProgressAppearance::Circular);
    }
    bar.disabled(mods.disabled).build(tokens)
}

/// `Banner { title, message, variant, action, icon }`: `icon` replaces the status icon.
pub(in crate::registry) fn banner(props: Props<'_>, tokens: &dyn Tokens) -> LayoutNode {
    use nexus_widget_banner::{Banner, BannerVariant};
    let variant = match text(props, "variant").as_deref() {
        Some("success") => BannerVariant::Success,
        Some("warning") => BannerVariant::Warning,
        Some("destructive") => BannerVariant::Destructive,
        _ => BannerVariant::Info,
    };
    let mut banner = Banner::new().variant(variant);
    if let Some(title) = text(props, "title") {
        banner = banner.title(title);
    }
    if let Some(message) = text(props, "message") {
        banner = banner.message(message);
    }
    if let Some(action) = text(props, "action") {
        banner = banner.action(action);
    }
    // The design's status icon: 18 px in the status colour.
    if let Some(icon) = icon_node(props, 18, variant.role(), tokens) {
        banner = banner.icon(icon);
    }
    banner.build(tokens)
}

/// `Toast { message, variant, action, icon }`: `icon` leads the message in the accent (the status
/// colour for a status variant).
pub(in crate::registry) fn toast(props: Props<'_>, tokens: &dyn Tokens) -> LayoutNode {
    use nexus_widget_toast::{Toast, ToastVariant};
    let mut toast = Toast::new(prop(props, "message").map(value_text).unwrap_or_default());
    toast = toast.variant(match text(props, "variant").as_deref() {
        Some("success") => ToastVariant::Success,
        Some("warning") => ToastVariant::Warning,
        Some("destructive") => ToastVariant::Destructive,
        _ => ToastVariant::Default,
    });
    if let Some(action) = text(props, "action") {
        toast = toast.action(action);
    }
    if let Some(icon) = icon_node(props, 16, toast.icon_role(), tokens) {
        toast = toast.icon(icon);
    }
    toast.build(tokens)
}

/// `Slider { value, variant, icon, tint, ticks }`. The default `variant: "control"` is the
/// Control Center bar (the glyph rides in the white fill; an explicit `tint` colours the fill);
/// `variant: "thumb"` is the thin slider (6 px track, tint fill with its glow, white thumb and
/// optional tick marks). Both take taps as the fraction across the track.
pub(in crate::registry) fn slider(
    props: Props<'_>,
    mods: &Mods,
    tokens: &dyn Tokens,
) -> LayoutNode {
    use nexus_widget_slider::{Slider, SliderVariant};
    let value = match prop(props, "value") {
        Some(Value::Int(v)) => (*v).clamp(0, 100) as u8,
        _ => 0,
    };
    let thumb = text(props, "variant").as_deref() == Some("thumb");
    let mut slider = Slider::new().value(value).variant(if thumb {
        SliderVariant::Thumb
    } else {
        SliderVariant::Control
    });
    if let Some(tint) = tint(props) {
        slider = slider.tint(tint);
    }
    if let Some(Value::Int(n)) = prop(props, "ticks") {
        slider = slider.ticks((*n).clamp(0, 50) as u8);
    }
    // The glyph embedded in the control bar's fill (`icon: "sun.max"`): `island` in every theme,
    // because the fill it sits on is white in every theme.
    if !thumb {
        if let Some(glyph) = icon_node(props, 14, ColorToken::Island, tokens) {
            slider = slider.leading(glyph);
        }
    }
    if mods.disabled {
        slider = slider.state(InteractionState::Disabled);
    }
    slider.build(tokens)
}
