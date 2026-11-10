// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The control widgets of the DSL catalog (`Button`, `Toggle`, `Checkbox`; the display, feedback
//! and input arms live in `display` and `fields`) and the props every
//! control shares: `size` (`xs`…`xl`, the control size) and `tint` (a colour role by name).
//!
//! CONTEXT: TASK-0353 P7 — the kit widgets gained the design's options (look, size, tint, role,
//! selected); these arms make them reachable from a page. Unknown values fall back to the
//! widget's default, so a typo renders the default control instead of nothing.

use super::{tokens::color_token, value_text, Mods};
use crate::store::Value;
use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::LayoutNode;
use nexus_style::control::Role;
use nexus_style::InteractionState;
use nexus_theme_tokens::{ColorToken, ControlSize, Tokens};
use nexus_widget_button::{ButtonShape, ButtonVariant, GlassButton};

mod display;
mod fields;
mod rows;
pub(super) use display::{badge, banner, card, progress_bar, skeleton, slider, spinner, toast};
pub(super) use fields::{search_bar, select, text_field};
pub(super) use rows::{avatar, chip, list_item, toolbar};

/// A prop lookup over the widget's `(name, value)` pairs.
pub(super) type Props<'a> = &'a [(String, Value)];

fn prop<'a>(props: Props<'a>, name: &str) -> Option<&'a Value> {
    props.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

fn flag(props: Props<'_>, name: &str) -> bool {
    matches!(prop(props, name), Some(Value::Bool(true)))
}

fn text(props: Props<'_>, name: &str) -> Option<String> {
    prop(props, name).map(value_text).filter(|t| !t.is_empty())
}

/// `size: "xs" | "sm" | "md" | "lg" | "xl"` — the control size.
pub(super) fn control_size(props: Props<'_>) -> Option<ControlSize> {
    match text(props, "size")?.as_str() {
        "xs" => Some(ControlSize::Xs),
        "sm" => Some(ControlSize::Sm),
        "md" => Some(ControlSize::Md),
        "lg" => Some(ControlSize::Lg),
        "xl" => Some(ControlSize::Xl),
        _ => None,
    }
}

/// `tint: "<role>"` — a colour role by its token name (`accent`, `danger`, `success`, …).
pub(super) fn tint(props: Props<'_>) -> Option<ColorToken> {
    tint_named(props, "tint")
}

/// A colour role by its token name from the prop `name` (`badgeTint: "success"`).
pub(super) fn tint_named(props: Props<'_>, name: &str) -> Option<ColorToken> {
    color_token(&text(props, name)?)
}

fn state(mods: &Mods) -> InteractionState {
    if mods.disabled {
        InteractionState::Disabled
    } else {
        InteractionState::Default
    }
}

/// `Button { label, variant, size, shape, tint, role, selected, fullWidth }`. The default look is
/// the design's glass; `variant: "prominent"` is the primary action (the filled look). The legacy
/// `.bg(danger)` / `.bg(fill)` still pick the destructive / secondary looks. Structure: root →
/// content (0) → label (0) + declared children (1+), which `child_path` mirrors — so the arm
/// takes no icon prop (a leading icon would shift the children).
pub(super) fn button(
    props: Props<'_>,
    mods: &Mods,
    tokens: &dyn Tokens,
    children: Vec<LayoutNode>,
) -> LayoutNode {
    let mut button = GlassButton::new().state(state(mods)).selected(flag(props, "selected"));
    let variant = match text(props, "variant").as_deref() {
        Some("glass") => Some(ButtonVariant::Glass),
        Some("ghost") | Some("plain") => Some(ButtonVariant::Ghost),
        Some("secondary") => Some(ButtonVariant::Secondary),
        Some("tinted") => Some(ButtonVariant::Tinted),
        Some("filled") => Some(ButtonVariant::Filled),
        Some("prominent") => {
            button = button.prominent(true);
            None
        }
        Some("destructive") => Some(ButtonVariant::Destructive),
        _ => match mods.bg {
            Some(ColorToken::Danger) => Some(ButtonVariant::Destructive),
            Some(ColorToken::Fill) => Some(ButtonVariant::Secondary),
            _ => None,
        },
    };
    if let Some(variant) = variant {
        button = button.variant(variant);
    }
    if flag(props, "prominent") {
        button = button.prominent(true);
    }
    if let Some(size) = control_size(props) {
        button = button.size(size);
    }
    match text(props, "shape").as_deref() {
        Some("rounded") => button = button.shape(ButtonShape::Rounded),
        Some("circle") => button = button.shape(ButtonShape::Circle),
        _ => {}
    }
    if let Some(tint) = tint(props) {
        button = button.tint(tint);
    }
    let role = match text(props, "role").as_deref() {
        Some("destructive") => Some(Role::Destructive),
        Some("confirm") => Some(Role::Confirm),
        Some("cancel") => Some(Role::Cancel),
        Some("close") => Some(Role::Close),
        _ => None,
    };
    if let Some(role) = role {
        button = button.role(role);
    }
    if flag(props, "fullWidth") {
        button = button.full_width(true);
    }
    button
        .label(prop(props, "label").map(value_text).unwrap_or_default())
        .children(children)
        .build(tokens)
}

/// `Toggle { checked, size, tint, appearance: "switch" | "button" }`. `label:` stays the
/// accessible name (a row draws its own title), so the arm does not render it.
pub(super) fn toggle(props: Props<'_>, mods: &Mods, tokens: &dyn Tokens) -> LayoutNode {
    use nexus_widget_toggle::{GlassToggle, ToggleAppearance};
    let mut toggle = GlassToggle::new().checked(flag(props, "checked")).state(state(mods));
    if let Some(size) = control_size(props) {
        toggle = toggle.size(size);
    }
    if let Some(tint) = tint(props) {
        toggle = toggle.tint(tint);
    }
    if text(props, "appearance").as_deref() == Some("button") {
        toggle = toggle.appearance(ToggleAppearance::Button);
    }
    toggle.build(tokens)
}

/// `Checkbox { checked, indeterminate, size, tint }`.
pub(super) fn checkbox(props: Props<'_>, mods: &Mods, tokens: &dyn Tokens) -> LayoutNode {
    use nexus_widget_checkbox::GlassCheckbox;
    let mut checkbox = GlassCheckbox::new()
        .checked(flag(props, "checked"))
        .indeterminate(flag(props, "indeterminate"))
        .state(state(mods));
    if let Some(size) = control_size(props) {
        checkbox = checkbox.size(size);
    }
    if let Some(tint) = tint(props) {
        checkbox = checkbox.tint(tint);
    }
    checkbox.build(tokens)
}
