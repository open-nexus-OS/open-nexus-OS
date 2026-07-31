// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! RFC-0086 shell path on the host: mount the REAL desktop shell, run the
//! root effects (enumerate), then fire the by-name `WindowsChanged` trigger
//! the way the app-host does when windowd pushes a window set — and pin that
//! BOTH steps keep the app list rendered. The device showed an empty
//! workspace + taskbar after the feed landed, which is exactly what a failed
//! re-emit looks like.

use nexus_dsl_runtime::{IdentityLocale, View};

struct Registry {
    id_sym: u32,
    label_sym: u32,
    icon_sym: u32,
    icon_top_sym: u32,
    icon_bottom_sym: u32,
    icon_art_sym: u32,
    running_sym: u32,
    enumerates: usize,
}

impl nexus_dsl_runtime::EffectHost for Registry {
    fn call(
        &mut self,
        svc: &str,
        method: &str,
        _a: &[nexus_dsl_runtime::Value],
        _t: u32,
    ) -> Result<nexus_dsl_runtime::Value, u32> {
        use nexus_dsl_runtime::Value;
        if (svc, method) == ("bundlemgr", "enumerate") {
            self.enumerates += 1;
            let row = |id: &str, label: &str| {
                let mut fields = vec![
                    (self.id_sym, Value::Str(id.into())),
                    (self.label_sym, Value::Str(label.into())),
                    (self.icon_sym, Value::Str("star".into())),
                    (self.icon_top_sym, Value::Str("#4ade80".into())),
                    (self.icon_bottom_sym, Value::Str("#15803d".into())),
                    (self.icon_art_sym, Value::Str(String::new())),
                    (self.running_sym, Value::Bool(false)),
                ];
                fields.sort_by_key(|(sym, _)| *sym);
                Value::Record(fields)
            };
            return Ok(Value::List(vec![row("calculator", "Calculator"), row("chat", "Chat")]));
        }
        Err(0)
    }
}

fn tile_count(view: &View) -> usize {
    nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(1280),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes
        .iter()
        .filter(|b| b.rect.width.as_i32() == 56 && b.rect.height.as_i32() == 56)
        .count()
}

#[test]
fn windows_changed_trigger_keeps_the_app_list_rendered() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop-shell");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("compiles");
    let symbols: Vec<String> =
        nexus_dsl_runtime::Runtime::mount(&nxir).expect("mounts").symbols().to_vec();
    let sym = |n: &str| symbols.iter().position(|s| s == n).expect(n) as u32;
    let mut host = Registry {
        id_sym: sym("id"),
        label_sym: sym("label"),
        icon_sym: sym("icon"),
        icon_top_sym: sym("iconTop"),
        icon_bottom_sym: sym("iconBottom"),
        icon_art_sym: sym("iconArt"),
        running_sym: sym("running"),
        enumerates: 0,
    };
    let device = nexus_dsl_runtime::FixtureEnv::desktop();
    let tokens = nexus_theme_tokens::BaseTokens;
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");

    // 1. Root effect (`Refresh` is dispatched by NOTHING) loads the list.
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("initial effects");
    assert_eq!(host.enumerates, 1, "the root enumerate must run at mount");
    let after_mount = tile_count(&view);
    assert!(after_mount >= 2, "workspace tiles after mount, got {after_mount}");

    // 2. The window feed lands: the host fires the by-name trigger.
    let damage = view
        .fire_trigger(&tokens, &device, &locale, &mut host, "WindowsChanged")
        .expect("WindowsChanged dispatches");
    assert!(damage.is_some(), "WindowsChanged must re-emit");
    assert_eq!(host.enumerates, 2, "the feed re-reads the registry");
    let after_feed = tile_count(&view);
    assert!(after_feed >= 2, "tiles must SURVIVE the feed re-emit, got {after_feed}");
}

/// The same feed, but asserted at LAYOUT level: the tiles must still occupy
/// real, on-screen boxes after a window opens.
///
/// `tiles_survive_the_window_feed` counts SCENE texts, which stay put even if
/// every box collapses to zero — the user-visible symptom ("the desktop icons
/// disappear when I open an app") is a geometry/paint failure that a scene
/// count cannot see.
#[test]
fn workspace_tiles_keep_real_boxes_after_a_window_opens() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../userspace/apps/desktop-shell");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("desktop-shell compiles");
    let symbols: Vec<String> =
        nexus_dsl_runtime::Runtime::mount(&nxir).expect("mounts").symbols().to_vec();
    let sym = |name: &str| {
        symbols.iter().position(|s| s == name).unwrap_or_else(|| panic!("symbol '{name}' missing"))
            as u32
    };
    let mut host = Registry {
        id_sym: sym("id"),
        label_sym: sym("label"),
        icon_sym: sym("icon"),
        icon_top_sym: sym("iconTop"),
        icon_bottom_sym: sym("iconBottom"),
        icon_art_sym: sym("iconArt"),
        running_sym: sym("running"),
        enumerates: 0,
    };
    let device = nexus_dsl_runtime::FixtureEnv::desktop();
    let tokens = nexus_theme_tokens::BaseTokens;
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("initial effects");

    let (w, h) = (1280i32, 800i32);
    let paintable = |view: &View<'_>| -> usize {
        let layout = nexus_layout::LayoutEngine::new()
            .layout_with_viewport(
                view.scene(),
                nexus_layout_types::FxPx::new(w),
                Some(nexus_layout_types::FxPx::new(h)),
                &nexus_text_baked::measure_text::BakedTextMeasure,
            )
            .expect("lays out");
        layout
            .boxes
            .iter()
            .filter(|b| {
                b.rect.width.0 > 0
                    && b.rect.height.0 > 0
                    && b.rect.x.0 < w
                    && b.rect.y.0 < h
                    && b.rect.x.0 + b.rect.width.0 > 0
                    && b.rect.y.0 + b.rect.height.0 > 0
            })
            .count()
    };

    let before = paintable(&view);
    assert!(before > 10, "desktop should paint real boxes at mount, got {before}");

    view.fire_trigger(&tokens, &device, &locale, &mut host, "WindowsChanged")
        .expect("WindowsChanged dispatches");
    let after = paintable(&view);
    assert!(
        after >= before,
        "opening a window must not erase the desktop: {before} -> {after} paintable boxes"
    );
}

/// Diagnostic: how many handlers does the real shell register when the
/// registry returns NOTHING? The device log showed exactly 11 with no desktop
/// icons, so this pins whether "icons gone" means "app list empty".
#[test]
fn handler_count_with_and_without_apps() {
    struct Empty;
    impl nexus_dsl_runtime::EffectHost for Empty {
        fn call(
            &mut self,
            svc: &str,
            method: &str,
            _a: &[nexus_dsl_runtime::Value],
            _t: u32,
        ) -> Result<nexus_dsl_runtime::Value, u32> {
            if (svc, method) == ("bundlemgr", "enumerate") {
                return Ok(nexus_dsl_runtime::Value::List(vec![]));
            }
            Err(0)
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../userspace/apps/desktop-shell");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("compiles");
    let symbols: Vec<String> =
        nexus_dsl_runtime::Runtime::mount(&nxir).expect("mounts").symbols().to_vec();
    let device = nexus_dsl_runtime::FixtureEnv::desktop();
    let tokens = nexus_theme_tokens::BaseTokens;
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = Empty;
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    println!("EMPTY REGISTRY -> {} handlers", view.handlers().len());
}
