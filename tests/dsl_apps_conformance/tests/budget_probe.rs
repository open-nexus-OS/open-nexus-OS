// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! Budget probes: the biggest DSL app must stay clear of the platform's
//! hard ceilings — the 4096-node layout cap (overrun = SILENT stale-layout
//! freeze via `relayout_retained`'s error swallow) and execd's 256KiB
//! payload VMO (`PAYLOAD_MAX_LEN`; the NXLC container = nxir + locale
//! packs). These assert with headroom so growth shows up as a red test,
//! not as a boot mystery.
mod common;
use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};

struct NoIo;
impl nexus_dsl_runtime::EffectHost for NoIo {
    fn call(&mut self, _: &str, _: &str, _: &[Value], _: u32) -> Result<Value, u32> {
        Ok(Value::Bool(true))
    }
}

#[test]
fn probe_settings_budgets() {
    for app in ["settings", "stash", "chat", "desktop-shell"] {
        let nxir = common::compile(app);
        println!("{app}: nxir={} bytes", nxir.len());
    }
    // The PAYLOAD ceiling is on the NXLC container (nxir + locale packs),
    // not the bare nxir — build it the way the bundle build does.
    let root = common::app_root("settings");
    let payload = nexus_dsl_core::compile_project_bundle(&root).expect("bundle");
    println!("settings: payload container={} bytes", payload.len());
    const PAYLOAD_MAX_LEN: usize = 512 * 1024; // execd/app-host contract
    assert!(
        payload.len() <= PAYLOAD_MAX_LEN * 9 / 10,
        "settings payload {} within 90% of the {}B ceiling — raise the \
         contract BEFORE the next section lands, not after the boot fails",
        payload.len(),
        PAYLOAD_MAX_LEN
    );
    let nxir: &'static [u8] = Box::leak(common::compile("settings").into_boxed_slice());
    let device = FixtureEnv::tablet("landscape");
    let symbols = common::program_symbols(nxir);
    let keys = common::program_i18n_keys(nxir);
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view =
        View::mount(nxir, &nexus_theme_tokens::BaseTokens, &device, &locale).expect("mounts");
    let mut host = NoIo;
    let mut probe = |view: &mut View, label: &str| {
        let boxes = common::layout_boxes(view);
        println!("settings[{label}]: boxes={} handlers={}", boxes.len(), view.handlers().len());
    };
    probe(&mut view, "landing browse/connections");
    for (ev, case, val) in [
        ("NavEvent", "GoOverview", None),
        ("NavEvent", "WinMenuPick", Some("general")),
        ("NavEvent", "WinMenuPick", Some("personal")),
    ] {
        let payload = val.map(|v| vec![Value::Str(v.into())]).unwrap_or_default();
        common::dispatch_with_keys(
            &mut view, &device, &mut host, &symbols, &keys, ev, case, payload,
        );
        probe(&mut view, case);
    }
    common::dispatch_with_keys(
        &mut view,
        &device,
        &mut host,
        &symbols,
        &keys,
        "NavEvent",
        "OpenSub",
        vec![Value::Str("appearance".into())],
    );
    probe(&mut view, "appearance");
    common::dispatch_with_keys(
        &mut view,
        &device,
        &mut host,
        &symbols,
        &keys,
        "WindowEvent",
        "WinMenu",
        vec![Value::Str("more.jump".into())],
    );
    probe(&mut view, "menu more.jump open");
    let boxes = common::layout_boxes(&view);
    assert!(
        boxes.len() < 2048,
        "settings' worst mode at {} boxes — half the 4096 engine cap is the \
         alarm line (overrun is a SILENT stale-layout freeze)",
        boxes.len()
    );
}

/// Dump the APP-menu overlay's box geometry + glass roots — the boot shows
/// the menu submitted as a layer but nothing visible.
#[test]
fn probe_app_menu_geometry() {
    let nxir: &'static [u8] = Box::leak(common::compile("settings").into_boxed_slice());
    let device = FixtureEnv::tablet("landscape");
    let symbols = common::program_symbols(nxir);
    let keys = common::program_i18n_keys(nxir);
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view =
        View::mount(nxir, &nexus_theme_tokens::BaseTokens, &device, &locale).expect("mounts");
    let mut host = NoIo;
    common::dispatch_with_keys(
        &mut view,
        &device,
        &mut host,
        &symbols,
        &keys,
        "WindowEvent",
        "WinMenu",
        vec![Value::Str("app".into())],
    );
    let boxes = common::layout_boxes(&view);
    println!("boxes={} ", boxes.len());
    for b in &boxes {
        let glass = !matches!(b.visual.material, nexus_layout_types::SurfaceMaterial::Opaque);
        if glass || b.rect.width.as_i32() == 0 || b.rect.height.as_i32() == 0 {
            println!(
                "id={} x={} y={} w={} h={} glass={} nested={} clip={:?}",
                b.node_id,
                b.rect.x.as_i32(),
                b.rect.y.as_i32(),
                b.rect.width.as_i32(),
                b.rect.height.as_i32(),
                glass,
                b.glass_nested,
                b.clip_rect.map(|c| (
                    c.x.as_i32(),
                    c.y.as_i32(),
                    c.width.as_i32(),
                    c.height.as_i32()
                ))
            );
        }
    }
    println!("--- last 45 boxes (absolutes last) ---");
    for b in boxes.iter().rev().take(45).collect::<Vec<_>>().iter().rev() {
        println!(
            "id={} x={} y={} w={} h={}",
            b.node_id,
            b.rect.x.as_i32(),
            b.rect.y.as_i32(),
            b.rect.width.as_i32(),
            b.rect.height.as_i32(),
        );
    }
    let texts = common::scene_texts(&view);
    println!(
        "menu shows Fenstermodus: {}",
        texts.iter().any(|t| t == "Window mode" || t == "Fenstermodus")
    );
}

/// Host-side timing split of one interactive cycle at the boot's surface
/// size (960x526): dispatch+re-emit, relayout, and a FULL raster pass.
#[test]
fn probe_interactive_cycle_cost() {
    use std::time::Instant;
    let nxir: &'static [u8] = Box::leak(common::compile("settings").into_boxed_slice());
    let device = FixtureEnv::tablet("landscape");
    let symbols = common::program_symbols(nxir);
    let keys = common::program_i18n_keys(nxir);
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view =
        View::mount(nxir, &nexus_theme_tokens::BaseTokens, &device, &locale).expect("mounts");
    let mut host = NoIo;

    let t = Instant::now();
    common::dispatch_with_keys(
        &mut view,
        &device,
        &mut host,
        &symbols,
        &keys,
        "WindowEvent",
        "WinMenu",
        vec![Value::Str("app".into())],
    );
    let d_dispatch = t.elapsed();

    let (w, h) = (960i32, 526i32);
    let t = Instant::now();
    let layout = nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(w),
            Some(nexus_layout_types::FxPx::new(h)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    let d_layout = t.elapsed();

    // Full raster: per row, paint every intersecting box (the render_rows
    // shape: pick + paint_row_picked).
    let t = Instant::now();
    let boxes = &layout.boxes;
    let pick: Vec<u32> = (0..boxes.len() as u32).collect();
    let mut row = vec![0u8; w as usize * 4];
    for y in 0..h {
        for px in row.chunks_exact_mut(4) {
            px.copy_from_slice(&[30, 30, 30, 168]);
        }
        let mut canvas = nexus_scene_raster::RowCanvas::new(&mut row, y, w);
        nexus_scene_raster::paint_row_picked(&mut canvas, boxes, &pick, None, None);
    }
    let d_paint = t.elapsed();
    println!(
        "cycle host: dispatch+reemit={:?} layout={:?} full-paint={:?} (x25 TCG ≈ {:?})",
        d_dispatch,
        d_layout,
        d_paint,
        (d_dispatch + d_layout + d_paint) * 25
    );
}

/// REGRESSION (engine scroll-viewport width rule): at the real 960x620
/// freeform frame the overview grid's intrinsic width must not leak through
/// the content pane's scroll viewport into the window row's flex negotiation.
/// Before the rule, the deficit path squeezed the fixed 240px sidebar to 228
/// (labels overlapped their icons) and the content pane overflowed the
/// window right edge by 13px — mode-dependent, browse was fine.
#[test]
fn probe_sidebar_width_mode_invariant() {
    let nxir: &'static [u8] = Box::leak(common::compile("settings").into_boxed_slice());
    let device = FixtureEnv::tablet("landscape");
    let symbols = common::program_symbols(nxir);
    let keys = common::program_i18n_keys(nxir);
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view =
        View::mount(nxir, &nexus_theme_tokens::BaseTokens, &device, &locale).expect("mounts");
    let mut host = NoIo;
    let lay = |view: &View| {
        nexus_layout::LayoutEngine::new()
            .layout_with_viewport(
                view.scene(),
                nexus_layout_types::FxPx::new(960),
                Some(nexus_layout_types::FxPx::new(620)),
                &nexus_text_baked::measure_text::BakedTextMeasure,
            )
            .expect("lays out")
            .boxes
    };
    // The twelve 38px sidebar rows, keyed by mode.
    let rows = |boxes: &[nexus_layout::LayoutBox]| -> Vec<(i32, i32)> {
        boxes
            .iter()
            .filter(|b| b.rect.height.as_i32() == 38 && b.rect.x.as_i32() < 300)
            .map(|b| (b.rect.x.as_i32(), b.rect.width.as_i32()))
            .collect()
    };
    let browse = lay(&view);
    let browse_rows = rows(&browse);
    assert_eq!(browse_rows.len(), 12, "twelve sidebar rows in browse mode");
    common::dispatch_with_keys(
        &mut view,
        &device,
        &mut host,
        &symbols,
        &keys,
        "NavEvent",
        "GoOverview",
        vec![],
    );
    let over = lay(&view);
    let over_rows = rows(&over);
    assert_eq!(
        browse_rows, over_rows,
        "sidebar row geometry must be mode-invariant (overview squeezed it)"
    );
    // Nothing in-flow may overflow the window: the pane's right edge kissing
    // past 960 was the same intrinsic-width leak seen from the other side.
    for b in &over {
        if b.clip_rect.is_none() {
            let right = b.rect.x.as_i32() + b.rect.width.as_i32();
            assert!(right <= 960, "box id={} right edge {} > 960", b.node_id, right);
        }
    }
}
