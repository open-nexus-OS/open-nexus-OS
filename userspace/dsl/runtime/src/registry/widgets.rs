// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Widget-kind builders: `kind symbol -> LayoutNode`.
//!
//! The other half of the registry seam (`tokens.rs` holds the vocabulary).
//! Every arm that is not a plain container calls the SAME builder the rest of
//! the OS uses from `userspace/ui/widgets/*` — the DSL widget IS the kit
//! widget, so a page and a hand-written surface cannot render it differently.

use super::controls;
use super::{plain_grid, plain_stack, spacing, text_node, value_px, value_text, Mods};
use crate::store::Value;
use alloc::string::String;
use nexus_layout_types::{Align, EdgeInsets, FxPx, Justify, LayoutNode, Material, Spacer};
use nexus_theme_tokens::{ColorToken, RadiusToken, SpaceToken, Tokens};

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
        "Card" => controls::card(props, mods, tokens, children),
        "Spacer" => LayoutNode::Spacer(Spacer::default()),
        // ── Design-system kit exposure (TASK-0073/0074): every arm calls the
        //    `userspace/ui/widgets/*` builder — the DSL widget IS the kit
        //    widget (one SSOT; visuals come from tokens, never ad-hoc). ──
        "Badge" => controls::badge(props, tokens),
        "Chip" => controls::chip(props, mods, tokens),
        "Avatar" => controls::avatar(props, mods, tokens),
        "Checkbox" => controls::checkbox(props, mods, tokens),
        "Slider" => controls::slider(props, mods, tokens),
        "Spinner" => controls::spinner(props, mods, tokens),
        "ProgressBar" => controls::progress_bar(props, mods, tokens),
        "Toast" => controls::toast(props, tokens),
        "Banner" => controls::banner(props, tokens),
        "Skeleton" => controls::skeleton(props, tokens),
        "ListItem" => controls::list_item(props, mods, tokens),
        "Toolbar" => controls::toolbar(props, tokens),
        "SearchBar" => controls::search_bar(props, mods, tokens),
        // Kit promotion: the CLOSED dropdown trigger (value + chevron). The open option panel is
        // deliberately not here — the kit crate documents it as an app-owned overlay, so a page
        // pairs this trigger with its own `.overlay()` list.
        "Select" => controls::select(props, mods, tokens),
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
        "TextField" => controls::text_field(props, mods, tokens),
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
            // The line weight in the design's own unit — the `strokeWidth` of the 24-unit grid
            // (`2` is the set's own; a menu's check is `2.6`), clamped to 1–4; in tenths.
            let stroke = match prop("stroke") {
                Some(Value::Int(s)) => Some((*s).clamp(1, 4) as u16 * 10),
                Some(Value::Fx(raw)) => {
                    Some(((i128::from(*raw) * 10 + (1 << 31)) >> 32).clamp(10, 40) as u16)
                }
                _ => None,
            };
            if let Some(lucide) = nexus_widget_icon::lucide_symbol_named(&name) {
                let icon = nexus_widget_icon::Icon::lucide(lucide)
                    .size(size)
                    .color(mods.fg.unwrap_or(ColorToken::LabelSecondary));
                return match stroke {
                    Some(tenths) => icon.stroke_tenths(tenths),
                    None => icon,
                }
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
