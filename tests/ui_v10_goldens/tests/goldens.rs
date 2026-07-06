// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Structural pixel goldens for the design-system core primitives in light/dark
//! and key interaction states. Regenerate with `UPDATE_GOLDENS=1`.

use nexus_layout_types::FxPx;
use nexus_style::InteractionState;
use nexus_theme_tokens::{BaseTokens, DarkTokens};
use nexus_widget_app_icon::{AppIcon, AppIconVariant};
use nexus_widget_badge::{Badge, BadgeVariant};
use nexus_widget_button::{ButtonVariant, GlassButton};
use nexus_widget_card::{CardLevel, GlassCard};
use nexus_widget_toggle::GlassToggle;
use ui_v10_goldens::{check_golden, swatch};

#[test]
fn glass_button_variants_and_states() {
    let t = BaseTokens;
    let btn = |v: ButtonVariant, s: InteractionState| {
        GlassButton::new().variant(v).state(s).content(swatch(20)).build(&t)
    };
    check_golden("button_default", &btn(ButtonVariant::Default, InteractionState::Default)).unwrap();
    check_golden("button_glass", &btn(ButtonVariant::Glass, InteractionState::Default)).unwrap();
    check_golden("button_destructive", &btn(ButtonVariant::Destructive, InteractionState::Default))
        .unwrap();
    check_golden("button_hover", &btn(ButtonVariant::Default, InteractionState::Hover)).unwrap();
    check_golden("button_pressed", &btn(ButtonVariant::Default, InteractionState::Pressed)).unwrap();
    check_golden("button_disabled", &btn(ButtonVariant::Default, InteractionState::Disabled))
        .unwrap();
    check_golden("button_focus", &btn(ButtonVariant::Default, InteractionState::Focused)).unwrap();
}

#[test]
fn glass_button_dark_theme() {
    let node = GlassButton::new().variant(ButtonVariant::Default).content(swatch(20)).build(&DarkTokens);
    check_golden("button_default_dark", &node).unwrap();
}

#[test]
fn badges() {
    let t = BaseTokens;
    check_golden(
        "badge_success",
        &Badge::new().variant(BadgeVariant::Success).content(swatch(16)).build(&t),
    )
    .unwrap();
    check_golden(
        "badge_outline",
        &Badge::new().variant(BadgeVariant::Outline).content(swatch(16)).build(&t),
    )
    .unwrap();
}

#[test]
fn toggles() {
    let t = BaseTokens;
    check_golden("toggle_on", &GlassToggle::new().checked(true).build(&t)).unwrap();
    check_golden("toggle_off", &GlassToggle::new().checked(false).build(&t)).unwrap();
}

#[test]
fn glass_cards() {
    check_golden(
        "card_panel_dark",
        &GlassCard::new().level(CardLevel::Panel).padding(FxPx::new(12)).child(swatch(40)).build(&DarkTokens),
    )
    .unwrap();
    check_golden(
        "card_subtle",
        &GlassCard::new().level(CardLevel::Subtle).padding(FxPx::new(8)).child(swatch(40)).build(&BaseTokens),
    )
    .unwrap();
}

#[test]
fn controls() {
    use nexus_widget_checkbox::GlassCheckbox;
    use nexus_widget_radio::Radio;
    use nexus_widget_segment::Segment;
    let t = BaseTokens;
    check_golden("checkbox_on", &GlassCheckbox::new().checked(true).build(&t)).unwrap();
    check_golden("checkbox_off", &GlassCheckbox::new().checked(false).build(&t)).unwrap();
    check_golden("radio_selected", &Radio::new().selected(true).build(&t)).unwrap();
    check_golden("radio_unselected", &Radio::new().selected(false).build(&t)).unwrap();
    check_golden(
        "segment",
        &Segment::new().active(1).options(vec![swatch(22), swatch(22), swatch(22)]).build(&t),
    )
    .unwrap();
}

#[test]
fn text_fields() {
    use nexus_widget_text_field::GlassTextField;
    let t = BaseTokens;
    check_golden(
        "textfield_default",
        &GlassTextField::new().label("E-Mail").placeholder("name@firma.de").build(&t),
    )
    .unwrap();
    check_golden(
        "textfield_error",
        &GlassTextField::new().label("E-Mail").value("x").error("Zu kurz").build(&t),
    )
    .unwrap();
}

#[test]
fn more_controls() {
    use nexus_widget_search_bar::SearchBar;
    use nexus_widget_slider::Slider;
    use nexus_widget_stepper::Stepper;
    let t = BaseTokens;
    check_golden("searchbar", &SearchBar::new().leading(swatch(16)).build(&t)).unwrap();
    check_golden("slider_60", &Slider::new().value(60).build(&t)).unwrap();
    check_golden("slider_0", &Slider::new().value(0).build(&t)).unwrap();
    check_golden(
        "stepper",
        &Stepper::new().dec(swatch(12)).value(swatch(20)).inc(swatch(12)).build(&t),
    )
    .unwrap();
}

#[test]
fn select_and_textarea() {
    use nexus_widget_select::Select;
    use nexus_widget_text_area::TextArea;
    let t = BaseTokens;
    check_golden("select_value", &Select::new().value("Deutsch").build(&t)).unwrap();
    check_golden("select_placeholder", &Select::new().placeholder("Sprache").build(&t)).unwrap();
    check_golden(
        "textarea",
        &TextArea::new().label("Notiz").rows(3).max_length(280).show_count(true).build(&t),
    )
    .unwrap();
}

#[test]
fn nav_chip_avatar() {
    use nexus_widget_avatar::{Avatar, AvatarStatus};
    use nexus_widget_chip::Chip;
    let t = BaseTokens;
    check_golden("chip_selected", &Chip::new("Design").selected(true).build(&t)).unwrap();
    check_golden("chip_outline", &Chip::new("Ungelesen").build(&t)).unwrap();
    check_golden("avatar_initials", &Avatar::new().initials("LK").size(48).build(&t)).unwrap();
    check_golden(
        "avatar_square",
        &Avatar::new().initials("AB").size(48).square(true).status(AvatarStatus::Online).build(&t),
    )
    .unwrap();
}

#[test]
fn nav_bars() {
    use nexus_widget_pagination::Pagination;
    use nexus_widget_tab_bar::{TabBar, TabItem};
    use nexus_widget_toolbar::Toolbar;
    let t = BaseTokens;
    check_golden("toolbar", &Toolbar::new().title("Einstellungen").trailing(swatch(20)).build(&t))
        .unwrap();
    check_golden(
        "tabbar",
        &TabBar::new()
            .active(0)
            .tabs(vec![TabItem::new("Start"), TabItem::new("Nachrichten"), TabItem::new("Profil")])
            .build(&t),
    )
    .unwrap();
    check_golden("pagination", &Pagination::new(5).page(2).build(&t)).unwrap();
}

#[test]
fn nav_containers() {
    use nexus_widget_accordion::{Accordion, AccordionItem};
    use nexus_widget_sidebar::{Sidebar, SidebarItem};
    use nexus_widget_tree_view::{TreeNode, TreeView};
    let t = BaseTokens;
    check_golden(
        "accordion",
        &Accordion::new(vec![
            AccordionItem::new("Allgemein", swatch(30)).open(true),
            AccordionItem::new("Datenschutz", swatch(30)),
        ])
        .build(&t),
    )
    .unwrap();
    check_golden(
        "sidebar",
        &Sidebar::new(vec![
            SidebarItem::header("Bibliothek"),
            SidebarItem::item("all", "Alle Dateien"),
            SidebarItem::item("shared", "Geteilt").badge(2),
        ])
        .active("all")
        .width(180)
        .build(&t),
    )
    .unwrap();
    check_golden(
        "treeview",
        &TreeView::new(vec![TreeNode::branch(
            "src",
            "src",
            vec![TreeNode::leaf("app", "app.rs"), TreeNode::leaf("lib", "lib.rs")],
        )
        .expanded(true)])
        .selected("app")
        .build(&t),
    )
    .unwrap();
}

#[test]
fn icons_and_rating() {
    use nexus_widget_icon::{Icon, Symbol};
    use nexus_widget_rating::Rating;
    use nexus_theme_tokens::ColorToken;
    let t = BaseTokens;
    check_golden("icon_plus", &Icon::new(Symbol::Plus).size(28).color(ColorToken::OnSurface).build(&t))
        .unwrap();
    check_golden("icon_star", &Icon::new(Symbol::Star).size(28).color(ColorToken::Warning).build(&t))
        .unwrap();
    check_golden("icon_close", &Icon::new(Symbol::Close).size(28).color(ColorToken::Danger).build(&t))
        .unwrap();
    check_golden("icon_chevron_right", &Icon::new(Symbol::ChevronRight).size(28).build(&t)).unwrap();
    check_golden("rating_3of5", &Rating::new().value(3).max(5).size(20).build(&t)).unwrap();
    // Imported Lucide symbols (multi-contour vectors).
    use nexus_widget_icon::LucideSymbol;
    check_golden("lucide_menu", &Icon::lucide(LucideSymbol::Menu).size(28).build(&t)).unwrap();
    check_golden("lucide_check", &Icon::lucide(LucideSymbol::Check).size(28).color(ColorToken::Success).build(&t)).unwrap();
    check_golden("lucide_arrow_right", &Icon::lucide(LucideSymbol::ArrowRight).size(28).color(ColorToken::Accent).build(&t)).unwrap();
}

#[test]
fn app_icons() {
    let t = BaseTokens;
    check_golden(
        "appicon_wrapped",
        &AppIcon::new().variant(AppIconVariant::Wrapped).content(swatch(24)).build(&t),
    )
    .unwrap();
    check_golden(
        "appicon_native",
        &AppIcon::new().variant(AppIconVariant::Native).content(swatch(24)).build(&t),
    )
    .unwrap();
}
