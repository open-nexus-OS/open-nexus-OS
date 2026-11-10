// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Only the surface on the wallpaper blurs (design artifacts: materials; TASK-0353 P7's gate "0
// nested blurs in the shell's frames"). The layout stamps `glass_nested` on every box under a
// glass ancestor and the compositor blurs only the roots, so a blurring material asked for
// INSIDE another surface renders as a plain tint — a design miss no other test would see. This
// walks the shell's frames — the desktop, every panel, both launchers, the touch sheet — and
// refuses a blurring material under glass. A nested `overlay` is exempt: nested, it is opaque by
// definition (`Material::opaque`), the floating action bar on a pane.

mod common;

use nexus_dsl_runtime::{EffectHost, FixtureEnv, IdentityLocale, Value, View};
use nexus_layout_types::Material;

/// Every service answers with an empty list (the launcher's root effect binds a collection).
struct Quiet;

impl EffectHost for Quiet {
    fn call(&mut self, _svc: &str, _method: &str, _args: &[Value]) -> Result<Value, u32> {
        Ok(Value::List(Vec::new()))
    }
}

/// The boxes that ask for a blur under a glass ancestor — after checking the frame HAS glass: a
/// blur root and boxes under it, or the walk would prove nothing.
fn nested_blurs(view: &View<'_>) -> Vec<String> {
    let boxes = common::layout_boxes(view);
    let roots =
        boxes.iter().filter(|b| !b.glass_nested && b.visual.material.is_some_and(Material::blurs));
    assert!(roots.count() > 0, "a shell frame without a blur root");
    assert!(boxes.iter().any(|b| b.glass_nested), "a shell frame with nothing under glass");
    boxes
        .iter()
        .filter(|b| b.glass_nested)
        .filter(|b| b.visual.material.is_some_and(|m| m.blurs() && m != Material::Overlay))
        .map(|b| format!("node {} {:?} at {:?}", b.node_id, b.visual.material, b.rect))
        .collect()
}

fn walk(device: FixtureEnv, panels: &[&str]) {
    let nxir: &'static [u8] = Box::leak(common::compile("desktop-shell").into_boxed_slice());
    let symbols = common::program_symbols(nxir);
    let keys = common::program_i18n_keys(nxir);
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(nxir, &nexus_theme_tokens::BaseTokens, &device, &locale)
        .expect("the shell mounts");
    let found = nested_blurs(&view);
    assert!(found.is_empty(), "the bare shell asks for a blur under glass: {found:?}");
    let mut host = Quiet;
    for panel in panels {
        common::dispatch_with_keys(
            &mut view,
            &device,
            &mut host,
            &symbols,
            &keys,
            "PanelEvent",
            "SetPanel",
            vec![Value::Str((*panel).to_string())],
        );
        let found = nested_blurs(&view);
        assert!(found.is_empty(), "panel `{panel}` asks for a blur under glass: {found:?}");
    }
}

#[test]
fn the_desktop_shell_never_asks_for_a_blur_inside_glass() {
    walk(
        FixtureEnv::desktop(),
        &[
            "control",
            "notifications",
            "calendar",
            "wifi",
            "sound",
            "battery",
            "launcher",
            "launcher-big",
        ],
    );
}

#[test]
fn the_touch_shell_never_asks_for_a_blur_inside_glass() {
    walk(
        FixtureEnv::tablet("landscape"),
        &["control", "notifications", "calendar", "launcher-big"],
    );
    walk(FixtureEnv::tablet("portrait"), &["control", "notifications", "launcher-big"]);
}
