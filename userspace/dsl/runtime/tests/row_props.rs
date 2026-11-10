// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! The DSL navigation and row widgets reach the kit's options (TASK-0353 P7, wave B, window
//! primitives): what a page says in props renders exactly what the kit builder renders for the
//! same options, and an unknown value falls back to the default.

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout_types::LayoutNode;
use nexus_style::InteractionState;
use nexus_theme_tokens::{BaseTokens, ColorToken, ControlSize};

fn mount(src: &str) -> View<'static> {
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    let nxir = nexus_dsl_core::lower_file(&file, &model, src).expect("lowers").nxir;
    let nxir: &'static [u8] = Box::leak(nxir.into_boxed_slice());
    let device = FixtureEnv::default();
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    View::mount(nxir, &BaseTokens, &device, &locale).expect("mounts")
}

fn contains(node: &LayoutNode, want: &LayoutNode) -> bool {
    if node == want {
        return true;
    }
    match node {
        LayoutNode::Stack(_, _, children) | LayoutNode::Grid(_, _, children) => {
            children.iter().any(|c| contains(c, want))
        }
        _ => false,
    }
}

fn renders(widget: &str, want: LayoutNode) {
    let view = mount(&format!("Page P {{ Stack {{ {widget} }} }}"));
    assert!(contains(view.scene(), &want), "{widget} renders the kit's node");
}

fn icon(name: &str, size: i32, color: ColorToken) -> LayoutNode {
    let symbol = nexus_widget_icon::lucide_symbol_named(name).expect("a symbol of the set");
    nexus_widget_icon::Icon::lucide(symbol).size(size).color(color).build(&BaseTokens)
}

#[test]
fn a_list_item_takes_icon_trailing_badge_selection_and_separator() {
    use nexus_widget_list_item::{ListItem, RowSeparator, SelectionStyle};
    let t = BaseTokens;
    renders(
        r#"ListItem { title: "Wi-Fi", subtitle: "Home", icon: "wifi", trailing: "On", showChevron: true }"#,
        ListItem::new("Wi-Fi")
            .subtitle("Home")
            .leading(icon("wifi", 18, ColorToken::LabelSecondary))
            .trailing_text("On")
            .show_chevron(true)
            .build(&t),
    );
    renders(
        r#"ListItem { title: "Mail", badge: "3", badgeTint: "accent", selected: true }"#,
        ListItem::new("Mail").badge("3").badge_tint(ColorToken::Accent).selected(true).build(&t),
    );
    renders(
        r#"ListItem { title: "Pick", selectable: true, selected: true, selectionStyle: "checkmark", separator: "inset" }"#,
        ListItem::new("Pick")
            .selectable(true)
            .selected(true)
            .selection_style(SelectionStyle::Checkmark)
            .separator(RowSeparator::Inset)
            .build(&t),
    );
    renders(
        r#"ListItem { title: "Order", reorderable: true, separator: "full" }"#,
        ListItem::new("Order").reorderable(true).separator(RowSeparator::Full).build(&t),
    );
    // A typo falls back to the default row.
    renders(
        r#"ListItem { title: "Plain", selectionStyle: "bogus", separator: "bogus", icon: "no-such-symbol" }"#,
        ListItem::new("Plain").build(&t),
    );
    // A disabled page row dims to the row's own 45 %.
    let view = mount(r#"Page P { Stack { ListItem { title: "Off" }.disabled(true) } }"#);
    assert!(
        contains(view.scene(), &ListItem::new("Off").disabled(true).build(&t)),
        "a disabled page row dims"
    );
}

#[test]
fn a_toolbar_takes_its_variant() {
    use nexus_widget_toolbar::{Toolbar, ToolbarVariant};
    let t = BaseTokens;
    renders(
        r#"Toolbar { title: "Files", variant: "transparent" }"#,
        Toolbar::new().title("Files").variant(ToolbarVariant::Transparent).build(&t),
    );
    renders(
        r#"Toolbar { title: "Files", subtitle: "3 items", centerTitle: true, variant: "bogus" }"#,
        Toolbar::new().title("Files").subtitle("3 items").center_title(true).build(&t),
    );
}

#[test]
fn an_avatar_takes_status_and_square() {
    use nexus_widget_avatar::{Avatar, AvatarStatus};
    let t = BaseTokens;
    renders(
        r#"Avatar { name: "Ada Lovelace", size: 48, status: "online" }"#,
        Avatar::new().name("Ada Lovelace").size(48).status(AvatarStatus::Online).build(&t),
    );
    renders(
        r#"Avatar { initials: "J", size: 34, square: true, status: "away" }"#,
        Avatar::new().initials("J").size(34).square(true).status(AvatarStatus::Away).build(&t),
    );
    renders(r#"Avatar { initials: "K", status: "bogus" }"#, Avatar::new().initials("K").build(&t));
}

#[test]
fn a_chip_takes_its_appearance() {
    use nexus_widget_chip::{Chip, ChipAppearance};
    let t = BaseTokens;
    renders(
        r#"Chip { label: "Mail", appearance: "outline", tint: "accent", size: "sm" }"#,
        Chip::new("Mail")
            .appearance(ChipAppearance::Outline)
            .tint(ColorToken::Accent)
            .size(ControlSize::Sm)
            .build(&t),
    );
    renders(
        r#"Chip { label: "Tag", appearance: "tinted" }"#,
        Chip::new("Tag").appearance(ChipAppearance::Tinted).build(&t),
    );
    renders(r#"Chip { label: "Tag", appearance: "bogus" }"#, Chip::new("Tag").build(&t));
    let view = mount(r#"Page P { Stack { Chip { label: "Off" }.disabled(true) } }"#);
    assert!(
        contains(view.scene(), &Chip::new("Off").state(InteractionState::Disabled).build(&t)),
        "a disabled page chip dims"
    );
}
