// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Widget-kind builders: `kind symbol -> LayoutNode`.
//!
//! The other half of the registry seam (`tokens.rs` holds the vocabulary).
//! Every arm that is not a plain container calls the SAME builder the rest of
//! the OS uses from `userspace/ui/widgets/*` — the DSL widget IS the kit
//! widget, so a page and a hand-written surface cannot render it differently.

use super::{material_token, plain_stack, radius, spacing, text_node, value_px, value_text, Mods};
use crate::store::Value;
use alloc::string::String;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FxPx, GlassLevel, Justify, LayoutNode, Spacer,
};
use nexus_theme_tokens::{ColorToken, MaterialToken, Tokens};

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
        "Panel" => {
            // Container primitive: a Stack with the panel-glass surface
            // pre-applied — explicit `.material/.rounded/.padding` win.
            let mut m = mods.clone();
            if m.material.is_none() {
                m.material = material_token("panel");
            }
            if m.rounded.is_none() {
                m.rounded = Some(radius("lg"));
            }
            if m.padding == EdgeInsets::zero() {
                m.padding = EdgeInsets::all(spacing(3));
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
            m.rounded = Some(radius("full"));
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
                if mods.padding == EdgeInsets::zero() { spacing(3) } else { mods.padding.top },
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
                _ => BadgeVariant::Default,
            };
            let badge = Badge::new().variant(variant);
            let fg = badge.foreground(tokens);
            let label_node = {
                let mods = Mods {
                    text_size: Some(nexus_theme_tokens::TypographyToken::Sm),
                    ..Default::default()
                };
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
            if let Some(nexus_layout_types::SurfaceMaterial::Glass(level)) = mods.material {
                avatar = avatar.material(match level {
                    GlassLevel::Panel => MaterialToken::Panel,
                    GlassLevel::Card => MaterialToken::Card,
                    GlassLevel::Subtle => MaterialToken::Subtle,
                    GlassLevel::Window => MaterialToken::Window,
                    GlassLevel::WindowPane => MaterialToken::WindowPane,
                    GlassLevel::WindowBar => MaterialToken::WindowBar,
                    GlassLevel::Overlay => MaterialToken::Overlay,
                });
            }
            if let Some(fg) = mods.fg {
                avatar = avatar.fg(fg);
            }
            if let Some(size) = mods.text_size {
                avatar = avatar.type_size(size);
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
                Some("sm") => FieldSize::Sm,
                Some("lg") => FieldSize::Lg,
                _ => FieldSize::Md,
            });
            if mods.disabled {
                field = field.state(nexus_style::InteractionState::Disabled);
            }
            field.secure(matches!(prop("secure"), Some(Value::Bool(true)))).build(tokens)
        }
        "Toggle" => {
            let checked = matches!(prop("checked"), Some(Value::Bool(true)));
            let mut toggle = nexus_widget_toggle::GlassToggle::new().checked(checked);
            if mods.disabled {
                toggle = toggle.state(nexus_style::InteractionState::Disabled);
            }
            toggle.build(tokens)
        }
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
