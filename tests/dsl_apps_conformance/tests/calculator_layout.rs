// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// The calculator's RESPONSIVE contract (design_handoff_calculator): at every
// window size the keys divide both axes exactly, the `0` spans two tracks, the
// keypad absorbs all leftover height, and the label type steps UP as the
// window grows. Breakpoints are deliberately absent — these are the tests that
// would fail if someone re-introduced them.

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};

struct NoHost;
impl nexus_dsl_runtime::EffectHost for NoHost {
    fn call(
        &mut self,
        _s: &str,
        _m: &str,
        _a: &[nexus_dsl_runtime::Value],
        _t: u32,
    ) -> Result<nexus_dsl_runtime::Value, u32> {
        Err(0)
    }
}

fn layout_at(w: i32, h: i32) -> nexus_layout::LayoutResult {
    let nxir = common::compile("calculator");
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::desktop();
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(w),
            Some(nexus_layout_types::FxPx::new(h)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
}

/// The four keys of a row: boxes sharing a `y`, at the four distinct `x`
/// positions, wide enough to be keys rather than glyphs.
fn row_key_widths(layout: &nexus_layout::LayoutResult, w: i32) -> Vec<Vec<i32>> {
    let mut rows: std::collections::BTreeMap<i32, Vec<(i32, i32)>> = Default::default();
    for b in &layout.boxes {
        // A key wrapper is a direct child of a keypad row: it spans roughly a
        // quarter of the window and is taller than a text line.
        if b.rect.width.0 > w / 8 && b.rect.width.0 < w && b.rect.height.0 > 24 {
            rows.entry(b.rect.y.0).or_default().push((b.rect.x.0, b.rect.width.0));
        }
    }
    rows.into_values()
        .filter(|cells| cells.len() >= 3)
        .map(|mut cells| {
            cells.sort();
            cells.dedup();
            cells.into_iter().map(|(_, w)| w).collect()
        })
        .collect()
}

#[test]
fn keys_divide_exactly_at_every_window_size() {
    for (w, h) in [(392, 616), (600, 500), (960, 640), (1280, 800)] {
        let layout = layout_at(w, h);
        let rows = row_key_widths(&layout, w);
        assert!(rows.len() >= 4, "expected the keypad rows at {w}x{h}, got {rows:?}");
        for cells in &rows {
            // The `0` row is 2+1+1; every other row is four equal cells.
            let uniform: Vec<i32> = cells.iter().copied().filter(|c| *c < w / 3).collect();
            if uniform.len() >= 2 {
                let (lo, hi) = (*uniform.iter().min().unwrap(), *uniform.iter().max().unwrap());
                assert!(hi - lo <= 1, "keys uneven at {w}x{h}: {cells:?}");
            }
        }
    }
}

/// The user's actual requirement: bigger window ⇒ bigger label, in steps.
#[test]
fn label_type_steps_up_with_the_window() {
    let sizes: Vec<i32> = [(392, 616), (700, 700), (1280, 800)]
        .iter()
        .map(|(w, h)| {
            let layout = layout_at(*w, *h);
            // KEY labels only. The display numeral is pinned at the handoff's
            // 52px by its own fixed-height card, so including it would report
            // "52, 52, 52" and hide whether the keys grew at all.
            let keypad_top = layout
                .boxes
                .iter()
                .find(|b| (104..200).contains(&b.rect.height.0) && b.rect.width.0 > 100)
                .map(|b| b.rect.y.0 + b.rect.height.0)
                .expect("the display card");
            layout
                .boxes
                .iter()
                .filter(|b| b.rect.y.0 > keypad_top)
                .filter_map(|b| b.text_px)
                .map(|px| {
                    nexus_text_baked::FontSize::nearest(
                        px.as_i32(),
                        nexus_text_baked::Weight::Regular,
                    )
                    .px()
                })
                .max()
                .expect("the keypad must fit its labels")
        })
        .collect();
    for pair in sizes.windows(2) {
        assert!(pair[1] >= pair[0], "type must not shrink as the window grows: {sizes:?}");
    }
    assert!(sizes.last() > sizes.first(), "type must actually grow: {sizes:?}");
}

/// The display keeps its handoff minimum; the keypad takes everything else, so
/// growing the window grows the KEYS.
#[test]
fn the_keypad_absorbs_the_extra_height() {
    let small = layout_at(392, 616);
    let tall = layout_at(392, 900);
    let display_h = |l: &nexus_layout::LayoutResult| {
        l.boxes.iter().map(|b| b.rect.height.0).filter(|h| *h >= 104).min().unwrap_or(0)
    };
    assert_eq!(
        display_h(&small),
        display_h(&tall),
        "the display must not absorb the extra height — the keys should"
    );
}

/// A readable record of what the responsive contract actually delivers.
#[test]
fn measured_ladder() {
    for (w, h) in [(392, 616), (600, 500), (960, 640), (1280, 800)] {
        let l = layout_at(w, h);
        let key = l
            .boxes
            .iter()
            .find(|b| {
                b.rect.y.0 > 130
                    && b.rect.width.0 > w / 8
                    && b.rect.width.0 < w / 2
                    && b.rect.height.0 > 24
            })
            .map(|b| (b.rect.width.0, b.rect.height.0))
            .unwrap();
        let label = l
            .boxes
            .iter()
            .filter(|b| b.rect.y.0 > 130)
            .filter_map(|b| b.text_px)
            .map(|p| {
                nexus_text_baked::FontSize::nearest(p.as_i32(), nexus_text_baked::Weight::Regular)
                    .px()
            })
            .max()
            .unwrap();
        println!("window {w}x{h}: key {}x{} -> label {label}px", key.0, key.1);
    }
}
