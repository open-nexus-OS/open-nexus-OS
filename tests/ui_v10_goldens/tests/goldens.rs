// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Structural pixel goldens for the design-system core primitives in light and dark
//! and key interaction states. Every group runs in both themes: the light test keeps the
//! golden name, the dark twin appends `_dark`. Regenerate with `UPDATE_GOLDENS=1`.

use nexus_layout_types::FxPx;
use nexus_style::InteractionState;
use nexus_theme_tokens::{BaseTokens, DarkTokens, Tokens};
use nexus_widget_app_icon::{AppIcon, AppIconVariant};
use nexus_widget_badge::{Badge, BadgeVariant};
use nexus_widget_button::{ButtonVariant, GlassButton};
use nexus_widget_card::{CardLevel, GlassCard};
use nexus_widget_toggle::GlassToggle;
use ui_v10_goldens::{check_golden, swatch};

fn glass_button_variants_and_states(t: &dyn Tokens, s: &str) {
    let btn = |v: ButtonVariant, s: InteractionState| {
        GlassButton::new().variant(v).state(s).content(swatch(20)).build(t)
    };
    check_golden(
        &format!("button_default{s}"),
        &btn(ButtonVariant::Default, InteractionState::Default),
    )
    .unwrap();
    check_golden(
        &format!("button_glass{s}"),
        &btn(ButtonVariant::Glass, InteractionState::Default),
    )
    .unwrap();
    check_golden(
        &format!("button_destructive{s}"),
        &btn(ButtonVariant::Destructive, InteractionState::Default),
    )
    .unwrap();
    check_golden(
        &format!("button_hover{s}"),
        &btn(ButtonVariant::Default, InteractionState::Hover),
    )
    .unwrap();
    check_golden(
        &format!("button_pressed{s}"),
        &btn(ButtonVariant::Default, InteractionState::Pressed),
    )
    .unwrap();
    check_golden(
        &format!("button_disabled{s}"),
        &btn(ButtonVariant::Default, InteractionState::Disabled),
    )
    .unwrap();
    check_golden(
        &format!("button_focus{s}"),
        &btn(ButtonVariant::Default, InteractionState::Focused),
    )
    .unwrap();
}

fn badges(t: &dyn Tokens, s: &str) {
    check_golden(
        &format!("badge_success{s}"),
        &Badge::new().variant(BadgeVariant::Success).content(swatch(16)).build(t),
    )
    .unwrap();
    check_golden(
        &format!("badge_outline{s}"),
        &Badge::new().variant(BadgeVariant::Outline).content(swatch(16)).build(t),
    )
    .unwrap();
}

fn toggles(t: &dyn Tokens, s: &str) {
    check_golden(&format!("toggle_on{s}"), &GlassToggle::new().checked(true).build(t)).unwrap();
    check_golden(&format!("toggle_off{s}"), &GlassToggle::new().checked(false).build(t)).unwrap();
}

fn glass_cards(t: &dyn Tokens, s: &str) {
    check_golden(
        &format!("card_panel{s}"),
        &GlassCard::new().level(CardLevel::Panel).padding(FxPx::new(12)).child(swatch(40)).build(t),
    )
    .unwrap();
    check_golden(
        &format!("card_subtle{s}"),
        &GlassCard::new().level(CardLevel::Subtle).padding(FxPx::new(8)).child(swatch(40)).build(t),
    )
    .unwrap();
}

fn controls(t: &dyn Tokens, s: &str) {
    use nexus_widget_checkbox::GlassCheckbox;
    use nexus_widget_radio::Radio;
    use nexus_widget_segment::Segment;
    check_golden(&format!("checkbox_on{s}"), &GlassCheckbox::new().checked(true).build(t)).unwrap();
    check_golden(&format!("checkbox_off{s}"), &GlassCheckbox::new().checked(false).build(t))
        .unwrap();
    check_golden(&format!("radio_selected{s}"), &Radio::new().selected(true).build(t)).unwrap();
    check_golden(&format!("radio_unselected{s}"), &Radio::new().selected(false).build(t)).unwrap();
    check_golden(
        &format!("segment{s}"),
        &Segment::new().active(1).options(vec![swatch(22), swatch(22), swatch(22)]).build(t),
    )
    .unwrap();
}

fn text_fields(t: &dyn Tokens, s: &str) {
    use nexus_widget_text_field::GlassTextField;
    check_golden(
        &format!("textfield_default{s}"),
        &GlassTextField::new().label("E-Mail").placeholder("name@firma.de").build(t),
    )
    .unwrap();
    check_golden(
        &format!("textfield_error{s}"),
        &GlassTextField::new().label("E-Mail").value("x").error("Zu kurz").build(t),
    )
    .unwrap();
}

fn more_controls(t: &dyn Tokens, s: &str) {
    use nexus_widget_search_bar::SearchBar;
    use nexus_widget_slider::Slider;
    use nexus_widget_stepper::Stepper;
    check_golden(&format!("searchbar{s}"), &SearchBar::new().leading(swatch(16)).build(t)).unwrap();
    check_golden(&format!("slider_60{s}"), &Slider::new().value(60).build(t)).unwrap();
    // Value 0 is the interesting one: the fill is the HANDLE, so it must still
    // be a visible cap rather than nothing at all.
    check_golden(&format!("slider_0{s}"), &Slider::new().value(0).build(t)).unwrap();
    // The embedded glyph rides inside the fill at a fixed inset — the fill
    // sweeps over it, so at a low value it is the fill that must still be
    // wide enough to hold it.
    check_golden(&format!("slider_icon{s}"), &Slider::new().value(35).leading(swatch(13)).build(t))
        .unwrap();
    check_golden(
        &format!("stepper{s}"),
        &Stepper::new().dec(swatch(12)).value(swatch(20)).inc(swatch(12)).build(t),
    )
    .unwrap();
}

fn select_and_textarea(t: &dyn Tokens, s: &str) {
    use nexus_widget_select::Select;
    use nexus_widget_text_area::TextArea;
    check_golden(&format!("select_value{s}"), &Select::new().value("Deutsch").build(t)).unwrap();
    check_golden(&format!("select_placeholder{s}"), &Select::new().placeholder("Sprache").build(t))
        .unwrap();
    check_golden(
        &format!("textarea{s}"),
        &TextArea::new().label("Notiz").rows(3).max_length(280).show_count(true).build(t),
    )
    .unwrap();
}

fn nav_chip_avatar(t: &dyn Tokens, s: &str) {
    use nexus_widget_avatar::{Avatar, AvatarStatus};
    use nexus_widget_chip::Chip;
    check_golden(&format!("chip_selected{s}"), &Chip::new("Design").selected(true).build(t))
        .unwrap();
    check_golden(&format!("chip_outline{s}"), &Chip::new("Ungelesen").build(t)).unwrap();
    check_golden(&format!("avatar_initials{s}"), &Avatar::new().initials("LK").size(48).build(t))
        .unwrap();
    check_golden(
        &format!("avatar_square{s}"),
        &Avatar::new().initials("AB").size(48).square(true).status(AvatarStatus::Online).build(t),
    )
    .unwrap();
}

fn nav_bars(t: &dyn Tokens, s: &str) {
    use nexus_widget_pagination::Pagination;
    use nexus_widget_tab_bar::{TabBar, TabItem};
    use nexus_widget_toolbar::Toolbar;
    check_golden(
        &format!("toolbar{s}"),
        &Toolbar::new().title("Einstellungen").trailing(swatch(20)).build(t),
    )
    .unwrap();
    check_golden(
        &format!("tabbar{s}"),
        &TabBar::new()
            .active(0)
            .tabs(vec![TabItem::new("Start"), TabItem::new("Nachrichten"), TabItem::new("Profil")])
            .build(t),
    )
    .unwrap();
    check_golden(&format!("pagination{s}"), &Pagination::new(5).page(2).build(t)).unwrap();
}

fn nav_containers(t: &dyn Tokens, s: &str) {
    use nexus_widget_accordion::{Accordion, AccordionItem};
    use nexus_widget_sidebar::{Sidebar, SidebarItem};
    use nexus_widget_tree_view::{TreeNode, TreeView};
    check_golden(
        &format!("accordion{s}"),
        &Accordion::new(vec![
            AccordionItem::new("Allgemein", swatch(30)).open(true),
            AccordionItem::new("Datenschutz", swatch(30)),
        ])
        .build(t),
    )
    .unwrap();
    check_golden(
        &format!("sidebar{s}"),
        &Sidebar::new(vec![
            SidebarItem::header("Bibliothek"),
            SidebarItem::item("all", "Alle Dateien"),
            SidebarItem::item("shared", "Geteilt").badge(2),
        ])
        .active("all")
        .width(180)
        .build(t),
    )
    .unwrap();
    check_golden(
        &format!("treeview{s}"),
        &TreeView::new(vec![TreeNode::branch(
            "src",
            "src",
            vec![TreeNode::leaf("app", "app.rs"), TreeNode::leaf("lib", "lib.rs")],
        )
        .expanded(true)])
        .selected("app")
        .build(t),
    )
    .unwrap();
}

fn icons_and_rating(t: &dyn Tokens, s: &str) {
    use nexus_theme_tokens::ColorToken;
    use nexus_widget_icon::{Icon, Symbol};
    use nexus_widget_rating::Rating;
    check_golden(
        &format!("icon_plus{s}"),
        &Icon::new(Symbol::Plus).size(28).color(ColorToken::Label).build(t),
    )
    .unwrap();
    check_golden(
        &format!("icon_star{s}"),
        &Icon::new(Symbol::Star).size(28).color(ColorToken::Warning).build(t),
    )
    .unwrap();
    check_golden(
        &format!("icon_close{s}"),
        &Icon::new(Symbol::Close).size(28).color(ColorToken::Danger).build(t),
    )
    .unwrap();
    check_golden(
        &format!("icon_chevron_right{s}"),
        &Icon::new(Symbol::ChevronRight).size(28).build(t),
    )
    .unwrap();
    check_golden(&format!("rating_3of5{s}"), &Rating::new().value(3).max(5).size(20).build(t))
        .unwrap();
    // Imported Lucide symbols (multi-contour vectors).
    use nexus_widget_icon::LucideSymbol;
    check_golden(&format!("lucide_menu{s}"), &Icon::lucide(LucideSymbol::Menu).size(28).build(t))
        .unwrap();
    check_golden(
        &format!("lucide_check{s}"),
        &Icon::lucide(LucideSymbol::Check).size(28).color(ColorToken::Success).build(t),
    )
    .unwrap();
    check_golden(
        &format!("lucide_arrow_right{s}"),
        &Icon::lucide(LucideSymbol::ArrowRight).size(28).color(ColorToken::Accent).build(t),
    )
    .unwrap();
}

fn app_icons(t: &dyn Tokens, s: &str) {
    check_golden(
        &format!("appicon_freestanding{s}"),
        &AppIcon::new().variant(AppIconVariant::Freestanding).content(swatch(24)).build(t),
    )
    .unwrap();
    check_golden(
        &format!("appicon_native{s}"),
        &AppIcon::new().variant(AppIconVariant::Native).content(swatch(24)).build(t),
    )
    .unwrap();
}

// ── Feedback group (TASK-0073 completion) ──

fn feedback_spinner_and_progress(t: &dyn Tokens, s: &str) {
    use nexus_widget_progress_bar::ProgressBar;
    use nexus_widget_spinner::Spinner;
    check_golden(&format!("spinner_default{s}"), &Spinner::new().build(t)).unwrap();
    check_golden(&format!("spinner_phase6{s}"), &Spinner::new().phase(6).build(t)).unwrap();
    // The dark twin predates the suffix scheme and keeps its name.
    let p64 = if s.is_empty() { "progress_64" } else { "progress_dark" };
    check_golden(p64, &ProgressBar::new().value(64).build(t)).unwrap();
    check_golden(
        &format!("progress_indeterminate{s}"),
        &ProgressBar::new().indeterminate().phase(40).build(t),
    )
    .unwrap();
}

fn feedback_toast_variants(t: &dyn Tokens, s: &str) {
    use nexus_widget_toast::{Toast, ToastVariant};
    check_golden(&format!("toast_default{s}"), &Toast::new("Datei gespeichert").build(t)).unwrap();
    check_golden(
        &format!("toast_destructive_action{s}"),
        &Toast::new("Verbindung verloren")
            .variant(ToastVariant::Destructive)
            .action("Erneut")
            .action_id("toast_retry")
            .build(t),
    )
    .unwrap();
}

fn feedback_skeletons(t: &dyn Tokens, s: &str) {
    use nexus_widget_skeleton::{Skeleton, SkeletonText};
    check_golden(&format!("skeleton_line{s}"), &Skeleton::new().width(200).height(16).build(t))
        .unwrap();
    check_golden(&format!("skeleton_circle{s}"), &Skeleton::new().height(48).circle().build(t))
        .unwrap();
    check_golden(&format!("skeleton_text{s}"), &SkeletonText::new().lines(3).build(t)).unwrap();
}

fn feedback_banner_variants(t: &dyn Tokens, s: &str) {
    use nexus_widget_banner::{Banner, BannerVariant};
    check_golden(
        &format!("banner_warning{s}"),
        &Banner::new()
            .variant(BannerVariant::Warning)
            .title("Speicher fast voll")
            .message("Noch 1,2 GB frei.")
            .action("Verwalten")
            .action_id("banner_manage")
            .build(t),
    )
    .unwrap();
    check_golden(
        &format!("banner_success{s}"),
        &Banner::new()
            .variant(BannerVariant::Success)
            .message("Synchronisierung abgeschlossen")
            .dismissible(true)
            .dismiss_id("banner_dismiss")
            .build(t),
    )
    .unwrap();
}

fn feedback_refresher_states(t: &dyn Tokens, s: &str) {
    use nexus_widget_refresher::Refresher;
    check_golden(
        &format!("refresher_pulling{s}"),
        &Refresher::new().progress(60).content(swatch(120)).build(t),
    )
    .unwrap();
    check_golden(
        &format!("refresher_refreshing{s}"),
        &Refresher::new().refreshing(true).content(swatch(120)).build(t),
    )
    .unwrap();
}

// Each group runs once per theme.
macro_rules! both_themes {
    ($($light:ident / $dark:ident => $body:ident),* $(,)?) => {$(
        #[test]
        fn $light() {
            $body(&BaseTokens, "");
        }
        #[test]
        fn $dark() {
            $body(&DarkTokens, "_dark");
        }
    )*};
}

both_themes! {
    glass_button_variants_and_states_light / glass_button_variants_and_states_dark => glass_button_variants_and_states,
    badges_light / badges_dark => badges,
    toggles_light / toggles_dark => toggles,
    glass_cards_light / glass_cards_dark => glass_cards,
    controls_light / controls_dark => controls,
    text_fields_light / text_fields_dark => text_fields,
    more_controls_light / more_controls_dark => more_controls,
    select_and_textarea_light / select_and_textarea_dark => select_and_textarea,
    nav_chip_avatar_light / nav_chip_avatar_dark => nav_chip_avatar,
    nav_bars_light / nav_bars_dark => nav_bars,
    nav_containers_light / nav_containers_dark => nav_containers,
    icons_and_rating_light / icons_and_rating_dark => icons_and_rating,
    app_icons_light / app_icons_dark => app_icons,
    feedback_spinner_and_progress_light / feedback_spinner_and_progress_dark => feedback_spinner_and_progress,
    feedback_toast_variants_light / feedback_toast_variants_dark => feedback_toast_variants,
    feedback_skeletons_light / feedback_skeletons_dark => feedback_skeletons,
    feedback_banner_variants_light / feedback_banner_variants_dark => feedback_banner_variants,
    feedback_refresher_states_light / feedback_refresher_states_dark => feedback_refresher_states,
}
