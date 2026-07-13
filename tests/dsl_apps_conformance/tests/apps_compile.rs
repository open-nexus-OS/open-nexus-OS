// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Gate zero: every shipped DSL app project compiles to canonical `.nxir` and
// mounts at the display size. Per-app interaction tests live in their own
// files (shell.rs / settings.rs / search.rs / chat.rs).

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};

fn app_root(app: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../userspace/apps").join(app)
}

fn compile_and_mount(app: &str) {
    let nxir = nexus_dsl_core::compile_project_dir(&app_root(app))
        .unwrap_or_else(|e| panic!("{app} compiles: {e}"));
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale)
        .unwrap_or_else(|e| panic!("{app} mounts: {e:?}"));
    let engine = nexus_layout::LayoutEngine::new();
    let layout = engine
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(1280),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .unwrap_or_else(|e| panic!("{app} lays out: {e:?}"));
    let sized =
        layout.boxes.iter().filter(|b| b.rect.width.as_i32() > 0 && b.rect.height.as_i32() > 0);
    assert!(sized.count() >= 3, "{app}: expected real geometry");
}

#[test]
fn desktop_shell_compiles_and_mounts() {
    compile_and_mount("desktop-shell");
}

#[test]
fn greeter_compiles_and_mounts() {
    compile_and_mount("greeter");
}

#[test]
fn counter_compiles_and_mounts() {
    compile_and_mount("counter");
}

/// The DSL animation binding (TASK-0062/0075): the counter page authors
/// `.effect(wiggle, …)` on the value text and `.animate(fadeScale, …)` on the
/// activity bar. Both intents must reach the mounted `View` — proof the
/// front-end stamps the decided motion modifiers (not a silent `_ => {}`).
#[test]
fn counter_emits_animation_intents() {
    use nexus_dsl_runtime::AnimKind;
    let nxir = nexus_dsl_core::compile_project_dir(&app_root("counter"))
        .unwrap_or_else(|e| panic!("counter compiles: {e}"));
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale)
        .unwrap_or_else(|e| panic!("counter mounts: {e:?}"));
    let anims = view.animations();
    assert!(
        anims.iter().any(|(_, i)| i.kind == AnimKind::Effect),
        "counter: expected the `.effect(wiggle)` intent, got {anims:?}"
    );
    assert!(
        anims.iter().any(|(_, i)| i.kind == AnimKind::Animate),
        "counter: expected the `.animate(fadeScale)` intent, got {anims:?}"
    );
    // Track B: the Skeleton kit widget emits a continuous Loop intent (no
    // modifier) so the host breathes its resting frame on the frame pulse.
    assert!(
        anims.iter().any(|(_, i)| i.kind == AnimKind::Loop),
        "counter: expected the Skeleton `Loop` intent, got {anims:?}"
    );
}

/// Regression contract for the app-host's scroll detection (probe/scroll.rs):
/// a page may contain CLIPPING widgets (the Skeleton's shimmer band is an
/// `Overflow::Hidden` stack → its boxes carry `clip_rect`) WITHOUT declaring
/// `.scroll(...)`. Scroll detection must key on an `Overflow::Scroll`
/// container, never on "any box with a clip" — the old heuristic misdetected
/// this exact page as scrollable and flipped it onto the banded
/// compositor-scroll surface path (which broke its animations).
#[test]
fn counter_clips_without_being_scrollable() {
    use nexus_layout_types::Overflow;
    let nxir = nexus_dsl_core::compile_project_dir(&app_root("counter"))
        .unwrap_or_else(|e| panic!("counter compiles: {e}"));
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale)
        .unwrap_or_else(|e| panic!("counter mounts: {e:?}"));
    let engine = nexus_layout::LayoutEngine::new();
    let layout = engine
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(320),
            Some(nexus_layout_types::FxPx::new(240)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .unwrap();
    let has_clip = layout.boxes.iter().any(|b| b.clip_rect.is_some());
    let has_scroll = layout
        .boxes
        .iter()
        .any(|b| matches!(b.overflow, Overflow::Scroll(_)));
    assert!(has_clip, "counter: the Skeleton must clip (the hazard's precondition)");
    assert!(!has_scroll, "counter: no `.scroll(...)` container — must NOT be scrollable");
}

#[test]
fn settings_compiles_and_mounts() {
    compile_and_mount("settings");
}

#[test]
fn search_compiles_and_mounts() {
    compile_and_mount("search");
}

#[test]
fn chat_compiles_and_mounts() {
    compile_and_mount("chat");
}
