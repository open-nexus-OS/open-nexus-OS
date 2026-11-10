// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The input arms of the DSL catalog (`TextField`, `SearchBar`, `Select`): the props the kit
//! widgets already carry, made reachable from a page.
//!
//! CONTEXT: TASK-0353 P7 (wave B). `TextField` builds `GlassTextField`, `SearchBar` the kit search
//! field, `Select` the closed trigger. The clear disc, the reveal eye and the search cancel
//! button are NOT exposed: each needs its own tap target, and a page cannot handle a tap on a
//! part of a widget yet (reported open, not drawn dead).

use super::{control_size, flag, prop, text, tint, Props};
use crate::registry::{value_text, Mods};
use nexus_layout_types::LayoutNode;
use nexus_style::InteractionState;
use nexus_theme_tokens::{ColorToken, Tokens};
use nexus_widget_text_field::{FieldAppearance, FieldMetrics};

fn state(mods: &Mods) -> InteractionState {
    if mods.disabled {
        InteractionState::Disabled
    } else {
        InteractionState::Default
    }
}

/// The leading icon of a field from `icon:` (the field's icon size, `labelSecondary`).
fn leading(
    props: Props<'_>,
    size: nexus_theme_tokens::ControlSize,
    tokens: &dyn Tokens,
) -> Option<LayoutNode> {
    let symbol = nexus_widget_icon::lucide_symbol_named(&text(props, "icon")?)?;
    Some(
        nexus_widget_icon::Icon::lucide(symbol)
            .size(FieldMetrics::of(size).icon)
            .color(ColorToken::LabelSecondary)
            .build(tokens),
    )
}

/// `TextField { value, label, placeholder, helper, error, validation, variant, appearance, shape,
/// size, tint, prefix, suffix, icon, readOnly, secure }`. `error` (a message) wins over
/// `validation` (the design's `state`, a keyword in the DSL); `validation: "error"` is the danger
/// border alone, `"success"` the green border, the check and a green helper. `appearance: "glass"` is the field on a wallpaper (alias of `variant: "glass"`);
/// `shape: "capsule"` the lock-screen exception.
pub(in crate::registry) fn text_field(
    props: Props<'_>,
    mods: &Mods,
    tokens: &dyn Tokens,
) -> LayoutNode {
    use nexus_widget_text_field::{FieldSize, FieldState, FieldVariant, GlassTextField};
    let mut field = GlassTextField::new();
    if let Some(label) = prop(props, "label").map(value_text) {
        field = field.label(label);
    }
    if let Some(value) = text(props, "value") {
        field = field.value(value);
    }
    if let Some(placeholder) = prop(props, "placeholder").map(value_text) {
        field = field.placeholder(placeholder);
    }
    // `error` replaces the helper line AND turns the border danger.
    if let Some(error) = text(props, "error") {
        field = field.error(error);
    } else if let Some(helper) = text(props, "helper") {
        field = field.helper(helper);
    }
    field = field.validation(match text(props, "validation").as_deref() {
        Some("error") => FieldState::Error,
        Some("success") => FieldState::Success,
        _ => FieldState::Default,
    });
    let variant = text(props, "variant").or_else(|| text(props, "appearance"));
    field = field.variant(match variant.as_deref() {
        Some("glass") | Some("pill") => FieldVariant::Glass,
        Some("bare") => FieldVariant::Bare,
        _ => FieldVariant::Boxed,
    });
    let size = control_size(props).unwrap_or(FieldSize::Md);
    field = field.size(size);
    if text(props, "shape").as_deref() == Some("capsule") {
        field = field.capsule(true);
    }
    if let Some(tint) = tint(props) {
        field = field.tint(tint);
    }
    if let Some(prefix) = text(props, "prefix") {
        field = field.prefix(prefix);
    }
    if let Some(suffix) = text(props, "suffix") {
        field = field.suffix(suffix);
    }
    if let Some(icon) = leading(props, size, tokens) {
        field = field.leading(icon);
    }
    field = field.read_only(flag(props, "readOnly")).state(state(mods));
    field.secure(flag(props, "secure")).build(tokens)
}

/// `SearchBar { value, placeholder, size, tint, appearance, loading }`: `appearance: "glass"` is
/// the field on a wallpaper; `loading: true` swaps the magnifier for the field spinner.
pub(in crate::registry) fn search_bar(
    props: Props<'_>,
    mods: &Mods,
    tokens: &dyn Tokens,
) -> LayoutNode {
    use nexus_widget_search_bar::SearchBar;
    let mut sb = SearchBar::new().loading(flag(props, "loading")).state(state(mods));
    if let Some(value) = prop(props, "value").map(value_text) {
        sb = sb.value(value);
    }
    if let Some(placeholder) = prop(props, "placeholder").map(value_text) {
        sb = sb.placeholder(placeholder);
    }
    if let Some(size) = control_size(props) {
        sb = sb.size(size);
    }
    if let Some(tint) = tint(props) {
        sb = sb.tint(tint);
    }
    if text(props, "appearance").as_deref() == Some("glass") {
        sb = sb.appearance(FieldAppearance::Glass);
    }
    sb.build(tokens)
}

/// `Select { value, placeholder, size, open }`: the closed trigger; `open: true` carries the
/// persistent fill layer and turns the chevron (the page shows its own option panel).
pub(in crate::registry) fn select(
    props: Props<'_>,
    mods: &Mods,
    tokens: &dyn Tokens,
) -> LayoutNode {
    use nexus_widget_select::Select;
    let mut sel = Select::new().open(flag(props, "open")).state(state(mods));
    if let Some(value) = prop(props, "value").map(value_text) {
        sel = sel.value(value);
    }
    if let Some(placeholder) = prop(props, "placeholder").map(value_text) {
        sel = sel.placeholder(placeholder);
    }
    if let Some(size) = control_size(props) {
        sel = sel.size(size);
    }
    sel.build(tokens)
}
