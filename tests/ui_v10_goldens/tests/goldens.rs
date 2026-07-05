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
