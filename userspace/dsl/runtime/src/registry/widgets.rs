// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Widget-kind builders: `kind symbol -> LayoutNode`.
//!
//! The other half of the registry seam (`tokens.rs` holds the vocabulary).
//! Every arm that is not a plain container calls the SAME builder the rest of
//! the OS uses from `userspace/ui/widgets/*` — the DSL widget IS the kit
//! widget, so a page and a hand-written surface cannot render it differently.

use super::controls::{self, control_size, tint};
use super::{plain_grid, plain_stack, spacing, text_node, value_px, value_text, Mods};
use crate::store::Value;
use alloc::string::String;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FxPx, Justify, LayoutNode, Material, Spacer,
};
use nexus_theme_tokens::{ColorToken, RadiusToken, SpaceToken, TextStyleToken, Tokens};

pub(super) fn build_widget_inner(
    kind: &str,
    props: &[(String, Value)],
    mods: &Mods,
    tokens: &dyn Tokens,
    children: alloc::vec::Vec<LayoutNode>,
) -> LayoutNode {
    let prop = |name: &str| props.iter().find(|(n, _)| n == name).map(|(_, v)| v);
    match kind {
        "Stack" | "List" => plain_stack(mods, tokens, children),
        "Grid" => {
            // Container primitive: the engine's REAL fixed-column grid.
            // `columns: n` = n equal 1fr tracks (row-major fill), `.gap()` =
            // column gap, `rowGap:` = row gap (px on the spacing grid; defaults
            // to the column gap inside the engine).
            let columns = match prop("columns") {
                Some(Value::Int(n)) => (*n).clamp(1, 12) as usize,
                _ => mods.columns.unwrap_or(1),
            };
            let row_gap = match prop("rowGap") {
                Some(Value::Int(n)) => Some(spacing(*n)),
                _ => mods.row_gap,
            };
            plain_grid(columns, row_gap, mods, tokens, children)
        }
        "Panel" => {
            // Container primitive: a Stack with the panel surface pre-applied (RFC-0116: the
            // outermost surface is `xl`, content inset `marginLayout`) — explicit
            // `.material/.rounded/.padding` win.
            let mut m = mods.clone();
            if m.material.is_none() {
                m.material = Some(Material::Panel);
            }
            if m.rounded.is_none() && m.corners.is_none() {
                m.rounded = Some(RadiusToken::Xl.px());
            }
            if m.padding == EdgeInsets::zero() {
                m.padding = EdgeInsets::all(SpaceToken::MarginLayout.px());
            }
            plain_stack(&m, tokens, children)
        }
        "Circle" => {
            // Container primitive: a perfectly round box — `size` (or
            // `.width`) pins a square, the corner radius is welded to full
            // and content centers on both axes. Round buttons, badges,
            // avatar-like elements; every modifier still applies.
            let size =
                prop("size").and_then(value_px).or(mods.width).unwrap_or_else(|| FxPx::new(40));
            let mut m = mods.clone();
            m.width = Some(size);
            m.height = Some(size);
            m.rounded = Some(RadiusToken::Full.px());
            if m.align.is_none() {
                m.align = Some(Align::Center);
            }
            if m.justify.is_none() {
                m.justify = Some(Justify::Center);
            }
            plain_stack(&m, tokens, children)
        }
        "Card" => {
            // Kit promotion: GlassCard (Panel + material tokens) is the SSOT.
            let mut card = nexus_widget_card::GlassCard::new().padding(
                if mods.padding == EdgeInsets::zero() {
                    SpaceToken::GapGroup.px()
                } else {
                    mods.padding.top
                },
            );
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
                Some("filled") => BadgeVariant::Filled,
                Some("tinted") => BadgeVariant::Tinted,
                _ => BadgeVariant::Default,
            };
            let mut badge = Badge::new().variant(variant);
            if let Some(tint) = tint(props) {
                badge = badge.tint(tint);
            }
            if let Some(size) = control_size(props) {
                badge = badge.size(size);
            }
            let fg = badge.foreground(tokens);
            let label_node = {
                let mods =
                    Mods { text_style: Some(TextStyleToken::Caption2), ..Default::default() };
                let mut node = text_node(label, &mods, tokens);
                if let LayoutNode::Text(text, _) = &mut node {
                    text.style.color = fg;
                    // A badge is mostly a count: its digits are tabular (RFC-0116 M-3).
                    text.style.tabular = true;
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
            if let Some(size) = control_size(props) {
                chip = chip.size(size);
            }
            if let Some(tint) = tint(props) {
                chip = chip.tint(tint);
            }
            if mods.disabled {
                chip = chip.state(nexus_style::InteractionState::Disabled);
            }
            chip.build(tokens)
        }
        "Avatar" => {
            use nexus_widget_avatar::Avatar;
            let mut avatar = Avatar::new();
            // `name` DERIVES the initials (the DSL has no string ops), so a
            // page binds the display name it already has; `initials` stays for
            // callers that carry them explicitly.
            if let Some(name) = prop("name").map(value_text).filter(|n| !n.is_empty()) {
                avatar = avatar.name(&name);
            } else if let Some(initials) = prop("initials").map(value_text) {
                avatar = avatar.initials(initials);
            }
            if let Some(Value::Int(size)) = prop("size") {
                avatar = avatar.size(*size as i32);
            }
            // The arm used to drop `mods` on the floor: `.material(card)` on an
            // Avatar was a silent no-op and every avatar was a flat grey disc.
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
        "Checkbox" => controls::checkbox(props, mods, tokens),
        "Slider" => {
            use nexus_widget_slider::Slider;
            let value = match prop("value") {
                Some(Value::Int(v)) => (*v).clamp(0, 100) as u8,
                _ => 0,
            };
            let mut slider = Slider::new().value(value);
            // The glyph embedded in the fill (`icon: "sun.max"`). Resolved
            // HERE, not in the widget crate: symbol names are a DSL-layer
            // vocabulary, and keeping it that way spares the slider a
            // dependency on the whole icon set. The glyph is `island` (black) in
            // every theme because the fill it sits on is white in every theme.
            let icon = prop("icon").map(value_text).unwrap_or_default();
            if let Some(lucide) = nexus_widget_icon::lucide_symbol_named(&icon) {
                slider = slider.leading(
                    nexus_widget_icon::Icon::lucide(lucide)
                        .size(14)
                        .color(ColorToken::Island)
                        .build(tokens),
                );
            }
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
            if let Some(color) = tint(props).or(mods.fg) {
                spinner = spinner.color(color);
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
            // `.fg()` picks the fill role, the way it already does for
            // `Spinner` above. Without it a battery meter is progress-blue:
            // the widget's default is the design's `Info`, and a page had no
            // way to say "this bar means charge, not progress".
            if let Some(color) = tint(props).or(mods.fg) {
                bar = bar.color(color);
            }
            if let Some(size) = control_size(props) {
                bar = bar.size(size);
            }
            if let Some(label) = prop("label").map(value_text).filter(|l| !l.is_empty()) {
                bar = bar.label(label);
            }
            if matches!(prop("showValue"), Some(Value::Bool(true))) {
                bar = bar.show_value(true);
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
            if let Some(size) = control_size(props) {
                sb = sb.size(size);
            }
            if let Some(tint) = tint(props) {
                sb = sb.tint(tint);
            }
            if mods.disabled {
                sb = sb.state(nexus_style::InteractionState::Disabled);
            }
            sb.build(tokens)
        }
        "Select" => {
            // Kit promotion: the CLOSED dropdown trigger (glass pill + value +
            // chevron). The open option panel is deliberately not here — the
            // kit crate documents it as an app-owned overlay, so a page pairs
            // this trigger with its own `.overlay()` list.
            use nexus_widget_select::Select;
            let mut sel = Select::new();
            if let Some(value) = prop("value").map(value_text) {
                sel = sel.value(value);
            }
            if let Some(ph) = prop("placeholder").map(value_text) {
                sel = sel.placeholder(ph);
            }
            if let Some(size) = control_size(props) {
                sel = sel.size(size);
            }
            if mods.disabled {
                sel = sel.state(nexus_style::InteractionState::Disabled);
            }
            sel.build(tokens)
        }
        "Breadcrumbs" => {
            // Kit promotion: the path trail. `items` is a `List<Str>`; a
            // non-list value degrades to the single crumb it stringifies to
            // rather than vanishing. The whole trail is ONE node, so a caller
            // that wants navigation wraps it and handles the tap itself —
            // per-crumb hit targets would need a multi-handler widget.
            use nexus_widget_breadcrumbs::Breadcrumbs;
            let items = match prop("items") {
                Some(Value::List(values)) => values.iter().map(value_text).collect(),
                Some(other) => alloc::vec![value_text(other)],
                None => alloc::vec::Vec::new(),
            };
            Breadcrumbs::new(items).build(tokens)
        }
        "Text" => {
            let value = prop("value").map(value_text).unwrap_or_default();
            text_node(value, mods, tokens)
        }
        "Button" => controls::button(props, mods, tokens, children),
        "TextField" => {
            // Kit promotion: GlassTextField (label + field + focus tokens).
            use nexus_widget_text_field::{FieldSize, FieldVariant, GlassTextField};
            let mut field = GlassTextField::new();
            if let Some(label) = prop("label").map(value_text) {
                field = field.label(label);
            }
            if let Some(value) = prop("value").map(value_text).filter(|v| !v.is_empty()) {
                field = field.value(value);
            }
            if let Some(placeholder) = prop("placeholder").map(value_text) {
                field = field.placeholder(placeholder);
            }
            // `error` replaces the helper line AND turns the border danger —
            // the state was implemented in the kit and unreachable from a page.
            if let Some(error) = prop("error").map(value_text).filter(|e| !e.is_empty()) {
                field = field.error(error);
            } else if let Some(helper) = prop("helper").map(value_text).filter(|h| !h.is_empty()) {
                field = field.helper(helper);
            }
            field = field.variant(match prop("variant").map(value_text).as_deref() {
                Some("glass") | Some("pill") => FieldVariant::Glass,
                Some("bare") => FieldVariant::Bare,
                _ => FieldVariant::Boxed,
            });
            field = field.size(match prop("size").map(value_text).as_deref() {
                Some("xs") => FieldSize::Xs,
                Some("sm") => FieldSize::Sm,
                Some("lg") => FieldSize::Lg,
                Some("xl") => FieldSize::Xl,
                _ => FieldSize::Md,
            });
            if mods.disabled {
                field = field.state(nexus_style::InteractionState::Disabled);
            }
            field.secure(matches!(prop("secure"), Some(Value::Bool(true)))).build(tokens)
        }
        "Toggle" => controls::toggle(props, mods, tokens),
        "Image" => {
            // Two baked artwork paths, chosen by the `source` scheme:
            //   * `"mime:<token>"` — a file-type icon from `nexus-mime-icons`.
            //     The token is an already-resolved stem (the app-host's fast
            //     path), a mime type, or a bare extension (RFC-0073 chain).
            //   * anything else — an app id whose bundle ships `assets/icon.svg`
            //     (manifest `icon_svg`), baked by `nexus-app-icons`.
            // `size` picks the baked size; the box is pinned square. Unknown
            // source = a transparent placeholder of the same size (layout stays
            // stable; the owning surface's fallback branch shows any fallback).
            let source = prop("source").map(value_text).unwrap_or_default();
            let size = match prop("size") {
                Some(Value::Int(s)) => (*s).clamp(8, 256) as i32,
                _ => 64,
            };
            let sprite = if let Some(token) = source.strip_prefix("mime:") {
                nexus_mime_icons::sprite_for_source(token, size as u32).map(|s| (s.size, s.rgba))
            } else {
                nexus_app_icons::sprite(&source, size as u32).map(|s| (s.size, s.rgba))
            };
            let px = FxPx::new(size);
            let mut node = plain_stack(
                &Mods { width: Some(px), height: Some(px), ..Mods::default() },
                tokens,
                alloc::vec![],
            );
            if let LayoutNode::Stack(_, visual, _) = &mut node {
                if let Some((sprite_size, rgba)) = sprite {
                    // The painter's shape dispatch runs under `background`;
                    // the blit arm ignores the color itself.
                    visual.background = Some(nexus_layout_types::Rgba8 { r: 0, g: 0, b: 0, a: 1 });
                    visual.shape = nexus_layout_types::ShapeKind::Raster {
                        w: sprite_size as u16,
                        h: sprite_size as u16,
                        rgba,
                    };
                }
            }
            node
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
                    .color(mods.fg.unwrap_or(ColorToken::LabelSecondary))
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
                    .color(mods.fg.unwrap_or(ColorToken::LabelSecondary))
                    .build(tokens),
                None => {
                    // Unknown symbol: honest tinted placeholder box.
                    let box_mods = Mods {
                        bg: Some(mods.fg.unwrap_or(ColorToken::LabelSecondary)),
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
