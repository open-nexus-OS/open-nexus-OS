// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The navigation and row arms of the DSL catalog (`ListItem`, `Toolbar`, `Avatar`, `Chip`): the
//! props the kit widgets already carry, made reachable from a page.
//!
//! CONTEXT: TASK-0353 P7 (wave B, window primitives). Every arm calls the SAME
//! `userspace/ui/widgets/*` builder the rest of the OS uses; an unknown value falls back to the
//! widget's default, so a typo renders the default widget instead of nothing. A prop that needs a
//! node (a row's `children`, a toolbar's clusters) or a static tap id (a chip's remove button, a
//! crumb) is not here: the catalog's widgets take none, and a handler id is a compile-time name.

use super::{control_size, flag, prop, text, tint, Props};
use crate::registry::{value_text, Mods};
use nexus_layout_types::LayoutNode;
use nexus_style::InteractionState;
use nexus_theme_tokens::{ColorToken, Tokens};

/// `ListItem { title, subtitle, icon, trailing, badge, badgeTint, showChevron, destructive,
/// selected, selectable, selectionStyle, reorderable, separator }`. `icon` is the leading glyph
/// (18 px, `labelSecondary`; the row inks it `danger` when destructive), `trailing` a value in
/// `labelSecondary`; `.disabled(true)` dims the row to 45 %.
pub(in crate::registry) fn list_item(
    props: Props<'_>,
    mods: &Mods,
    tokens: &dyn Tokens,
) -> LayoutNode {
    use nexus_widget_list_item::{ListItem, RowSeparator, SelectionStyle};
    let mut row = ListItem::new(prop(props, "title").map(value_text).unwrap_or_default())
        .show_chevron(flag(props, "showChevron"))
        .destructive(flag(props, "destructive"))
        .selected(flag(props, "selected"))
        .selectable(flag(props, "selectable"))
        .reorderable(flag(props, "reorderable"))
        .disabled(mods.disabled);
    if let Some(subtitle) = prop(props, "subtitle").map(value_text) {
        row = row.subtitle(subtitle);
    }
    if let Some(symbol) =
        text(props, "icon").as_deref().and_then(nexus_widget_icon::lucide_symbol_named)
    {
        row = row.leading(
            nexus_widget_icon::Icon::lucide(symbol)
                .size(18)
                .color(ColorToken::LabelSecondary)
                .build(tokens),
        );
    }
    if let Some(value) = text(props, "trailing") {
        row = row.trailing_text(value);
    }
    if let Some(badge) = text(props, "badge") {
        row = row.badge(badge);
    }
    if let Some(role) = super::tint_named(props, "badgeTint") {
        row = row.badge_tint(role);
    }
    match text(props, "selectionStyle").as_deref() {
        Some("checkmark") => row = row.selection_style(SelectionStyle::Checkmark),
        Some("circle") => row = row.selection_style(SelectionStyle::Circle),
        _ => {}
    }
    match text(props, "separator").as_deref() {
        Some("inset") => row = row.separator(RowSeparator::Inset),
        Some("full") => row = row.separator(RowSeparator::Full),
        _ => {}
    }
    row.build(tokens)
}

/// `Toolbar { title, subtitle, centerTitle, variant: "panel" | "transparent" }`: a title-only bar
/// (its leading and trailing clusters hold nodes, which a view cannot hand to a leaf widget).
pub(in crate::registry) fn toolbar(props: Props<'_>, tokens: &dyn Tokens) -> LayoutNode {
    use nexus_widget_toolbar::{Toolbar, ToolbarVariant};
    let mut bar = Toolbar::new().center_title(flag(props, "centerTitle"));
    if let Some(title) = prop(props, "title").map(value_text) {
        bar = bar.title(title);
    }
    if let Some(subtitle) = prop(props, "subtitle").map(value_text) {
        bar = bar.subtitle(subtitle);
    }
    match text(props, "variant").as_deref() {
        Some("transparent") => bar = bar.variant(ToolbarVariant::Transparent),
        Some("panel") => bar = bar.variant(ToolbarVariant::Panel),
        _ => {}
    }
    bar.build(tokens)
}

/// `Avatar { name, initials, size, status: "online" | "busy" | "away" | "offline", square }`; the
/// modifiers `.material`, `.fg` and `.textStyle` keep overriding the backing and the initials.
pub(in crate::registry) fn avatar(
    props: Props<'_>,
    mods: &Mods,
    tokens: &dyn Tokens,
) -> LayoutNode {
    use nexus_widget_avatar::{Avatar, AvatarStatus};
    let mut avatar = Avatar::new().square(flag(props, "square"));
    // `name` DERIVES the initials (the DSL has no string ops), so a page binds the display name
    // it already has; `initials` stays for callers that carry them explicitly.
    if let Some(name) = text(props, "name") {
        avatar = avatar.name(&name);
    } else if let Some(initials) = prop(props, "initials").map(value_text) {
        avatar = avatar.initials(initials);
    }
    if let Some(crate::store::Value::Int(size)) = prop(props, "size") {
        avatar = avatar.size(*size as i32);
    }
    match text(props, "status").as_deref() {
        Some("online") => avatar = avatar.status(AvatarStatus::Online),
        Some("busy") => avatar = avatar.status(AvatarStatus::Busy),
        Some("away") => avatar = avatar.status(AvatarStatus::Away),
        Some("offline") => avatar = avatar.status(AvatarStatus::Offline),
        _ => {}
    }
    // The arm once dropped `mods` on the floor: `.material(card)` on an Avatar was a silent
    // no-op and every avatar was a flat grey disc.
    if let Some(material) = mods.material {
        avatar = avatar.material(material);
    }
    if let Some(fg) = mods.fg {
        avatar = avatar.fg(fg);
    }
    if let Some(style) = mods.text_style {
        avatar = avatar.type_size(style);
    }
    avatar.build(tokens)
}

/// `Chip { label, selected, size, tint, appearance: "neutral" | "tinted" | "filled" | "outline" }`.
pub(in crate::registry) fn chip(props: Props<'_>, mods: &Mods, tokens: &dyn Tokens) -> LayoutNode {
    use nexus_widget_chip::{Chip, ChipAppearance};
    let mut chip = Chip::new(prop(props, "label").map(value_text).unwrap_or_default())
        .selected(flag(props, "selected"));
    if let Some(size) = control_size(props) {
        chip = chip.size(size);
    }
    if let Some(tint) = tint(props) {
        chip = chip.tint(tint);
    }
    match text(props, "appearance").as_deref() {
        Some("neutral") => chip = chip.appearance(ChipAppearance::Neutral),
        Some("tinted") => chip = chip.appearance(ChipAppearance::Tinted),
        Some("filled") => chip = chip.appearance(ChipAppearance::Filled),
        Some("outline") => chip = chip.appearance(ChipAppearance::Outline),
        _ => {}
    }
    if mods.disabled {
        chip = chip.state(InteractionState::Disabled);
    }
    chip.build(tokens)
}
