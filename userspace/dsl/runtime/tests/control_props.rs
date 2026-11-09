// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! The DSL control widgets reach the kit's options (TASK-0353 P7): a `Button`'s variant, size,
//! shape, tint and role, a `Toggle`'s and a `Checkbox`'s size and tint render exactly what the
//! kit builder renders for the same options; an unknown value falls back to the default.

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout_types::LayoutNode;
use nexus_style::control::Role;
use nexus_theme_tokens::{BaseTokens, ColorToken, ControlSize};
use nexus_widget_button::{ButtonShape, GlassButton};

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

#[test]
fn a_button_takes_the_kits_variant_size_shape_tint_and_role() {
    let t = BaseTokens;
    renders(
        r#"Button { label: "Go", variant: "prominent", size: "lg" }"#,
        GlassButton::new().prominent(true).size(ControlSize::Lg).label("Go").build(&t),
    );
    renders(
        r#"Button { label: "Del", variant: "tinted", tint: "danger", shape: "rounded" }"#,
        GlassButton::new()
            .variant(nexus_widget_button::ButtonVariant::Tinted)
            .tint(ColorToken::Danger)
            .shape(ButtonShape::Rounded)
            .label("Del")
            .build(&t),
    );
    renders(
        r#"Button { label: "", role: "close", size: "sm" }"#,
        GlassButton::new().role(Role::Close).size(ControlSize::Sm).label("").build(&t),
    );
}

#[test]
fn test_reject_an_unknown_control_value_falls_back_to_the_default() {
    let t = BaseTokens;
    renders(
        r#"Button { label: "Go", variant: "neon", size: "huge", tint: "notARole" }"#,
        GlassButton::new().label("Go").build(&t),
    );
}

#[test]
fn a_toggle_and_a_checkbox_take_size_and_tint() {
    let t = BaseTokens;
    renders(
        r#"Toggle { label: "Wi-Fi", checked: true, size: "sm", tint: "success" }"#,
        nexus_widget_toggle::GlassToggle::new()
            .checked(true)
            .size(ControlSize::Sm)
            .tint(ColorToken::Success)
            .build(&t),
    );
    renders(
        r#"Checkbox { label: "All", checked: false, indeterminate: true, size: "lg" }"#,
        nexus_widget_checkbox::GlassCheckbox::new()
            .indeterminate(true)
            .size(ControlSize::Lg)
            .build(&t),
    );
}
