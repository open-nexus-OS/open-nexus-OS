// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! The DSL display, feedback and input widgets reach the kit's options (TASK-0353 P7, wave B):
//! what a page says in props renders exactly what the kit builder renders for the same options,
//! and an unknown value falls back to the default.

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout_types::LayoutNode;
use nexus_style::InteractionState;
use nexus_theme_tokens::{BaseTokens, ColorToken, ControlSize, SpaceToken};

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
fn a_badge_takes_tint_size_dot_and_max() {
    use nexus_widget_badge::{Badge, BadgeVariant};
    let t = BaseTokens;
    // A tint without a variant fills in that role (the arm used to force the default look).
    renders(
        r#"Badge { label: "3", tint: "danger" }"#,
        Badge::new().tint(ColorToken::Danger).label("3").build(&t),
    );
    renders(
        r#"Badge { label: "120", max: 99, size: "lg", variant: "success" }"#,
        Badge::new()
            .variant(BadgeVariant::Success)
            .size(ControlSize::Lg)
            .max(99)
            .label("120")
            .build(&t),
    );
    renders(
        r#"Badge { dot: true, variant: "warning", size: "sm" }"#,
        Badge::new().dot(true).variant(BadgeVariant::Warning).size(ControlSize::Sm).build(&t),
    );
}

#[test]
fn a_card_takes_variant_and_material() {
    use nexus_widget_card::{CardLevel, GlassCard};
    let t = BaseTokens;
    renders(
        r#"Card { variant: "overlay" }"#,
        GlassCard::new().level(CardLevel::Overlay).padding(SpaceToken::GapGroup.px()).build(&t),
    );
    renders(
        r#"Card { variant: "dock", material: false }"#,
        GlassCard::new()
            .level(CardLevel::Dock)
            .material(false)
            .padding(SpaceToken::GapGroup.px())
            .build(&t),
    );
    // No props: the panel, as before.
    renders(r#"Card { }"#, GlassCard::new().padding(SpaceToken::GapGroup.px()).build(&t));
}

#[test]
fn skeleton_spinner_and_progress_take_their_options() {
    use nexus_widget_progress_bar::{ProgressAppearance, ProgressBar};
    let t = BaseTokens;
    renders(
        r#"Skeleton { width: 120, height: 20, radius: 6 }"#,
        nexus_widget_skeleton::Skeleton::new().width(120).height(20).radius(6).build(&t),
    );
    renders(
        r#"Spinner { size: "lg", tint: "accent" }"#,
        nexus_widget_spinner::Spinner::new()
            .flat()
            .control_size(ControlSize::Lg)
            .color(ColorToken::Accent)
            .build(&t),
    );
    renders(
        r#"Spinner { size: 20 }"#,
        nexus_widget_spinner::Spinner::new().flat().size(20).build(&t),
    );
    renders(
        r#"ProgressBar { value: 40, appearance: "circular", size: "lg", showValue: true }"#,
        ProgressBar::new()
            .value(40)
            .appearance(ProgressAppearance::Circular)
            .size(ControlSize::Lg)
            .show_value(true)
            .build(&t),
    );
    let view = mount("Page P { Stack { ProgressBar { value: 40 }.disabled(true) } }");
    assert!(
        contains(view.scene(), &ProgressBar::new().value(40).disabled(true).build(&t)),
        "a disabled page progress bar dims"
    );
}

#[test]
fn a_banner_and_a_toast_take_a_symbol_icon() {
    use nexus_widget_banner::{Banner, BannerVariant};
    use nexus_widget_toast::{Toast, ToastVariant};
    let t = BaseTokens;
    renders(
        r#"Banner { title: "Hi", variant: "warning", icon: "sun.max" }"#,
        Banner::new()
            .title("Hi")
            .variant(BannerVariant::Warning)
            .icon(icon("sun.max", 18, ColorToken::Warning))
            .build(&t),
    );
    renders(
        r#"Toast { message: "Saved", variant: "success", icon: "sun.max" }"#,
        Toast::new("Saved")
            .variant(ToastVariant::Success)
            .icon(icon("sun.max", 16, ColorToken::Success))
            .build(&t),
    );
    // An unknown symbol draws no icon (the status icon stays).
    renders(r#"Banner { title: "Hi", icon: "noSuchSymbol" }"#, Banner::new().title("Hi").build(&t));
}

#[test]
fn a_slider_takes_the_thumb_variant_tint_and_ticks() {
    use nexus_widget_slider::{Slider, SliderVariant};
    let t = BaseTokens;
    renders(
        r#"Slider { value: 40, variant: "thumb", tint: "success", ticks: 5 }.label("Level")"#,
        Slider::new()
            .value(40)
            .variant(SliderVariant::Thumb)
            .tint(ColorToken::Success)
            .ticks(5)
            .build(&t),
    );
    // The control bar stays the default; a tint colours its fill.
    renders(
        r#"Slider { value: 70, tint: "modeDesktop" }.label("Level")"#,
        Slider::new().value(70).tint(ColorToken::ModeDesktop).build(&t),
    );
}

#[test]
fn a_text_field_takes_validation_affixes_icon_shape_and_read_only() {
    use nexus_widget_text_field::{FieldSize, FieldState, GlassTextField};
    let t = BaseTokens;
    renders(
        r#"TextField { label: "Price", value: "12", prefix: "~", suffix: "EUR", validation: "success", size: "lg", tint: "success" }"#,
        GlassTextField::new()
            .label("Price")
            .value("12")
            .prefix("~")
            .suffix("EUR")
            .validation(FieldState::Success)
            .size(FieldSize::Lg)
            .tint(ColorToken::Success)
            .build(&t),
    );
    renders(
        r#"TextField { label: "Id", value: "7", readOnly: true, appearance: "glass", shape: "capsule" }"#,
        GlassTextField::new().label("Id").value("7").read_only(true).pill().build(&t),
    );
    let size = FieldSize::Md;
    renders(
        r#"TextField { label: "Find", icon: "sun.max" }"#,
        GlassTextField::new()
            .label("Find")
            .leading(
                nexus_widget_icon::Icon::lucide(
                    nexus_widget_icon::lucide_symbol_named("sun.max").expect("symbol"),
                )
                .size(nexus_widget_text_field::FieldMetrics::of(size).icon)
                .color(ColorToken::LabelSecondary)
                .build(&t),
            )
            .build(&t),
    );
    // An error message wins over a state.
    renders(
        r#"TextField { label: "Pin", error: "Too short", validation: "success" }"#,
        GlassTextField::new()
            .label("Pin")
            .error("Too short")
            .validation(FieldState::Success)
            .build(&t),
    );
}

#[test]
fn a_search_bar_and_a_select_take_their_options() {
    use nexus_widget_search_bar::SearchBar;
    use nexus_widget_select::Select;
    use nexus_widget_text_field::FieldAppearance;
    let t = BaseTokens;
    renders(
        r#"SearchBar { value: "ab", placeholder: "Find", appearance: "glass", loading: true }"#,
        SearchBar::new()
            .value("ab")
            .placeholder("Find")
            .appearance(FieldAppearance::Glass)
            .loading(true)
            .build(&t),
    );
    renders(
        r#"Select { value: "Deutsch", open: true, size: "sm", placeholder: "Sprache" }"#,
        Select::new()
            .value("Deutsch")
            .placeholder("Sprache")
            .open(true)
            .size(ControlSize::Sm)
            .build(&t),
    );
    let view =
        mount(r#"Page P { Stack { Select { value: "x", placeholder: "p" }.disabled(true) } }"#);
    assert!(contains(
        view.scene(),
        &Select::new().value("x").placeholder("p").state(InteractionState::Disabled).build(&t)
    ));
}
