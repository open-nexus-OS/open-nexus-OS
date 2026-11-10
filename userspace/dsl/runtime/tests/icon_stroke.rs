// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! An Icon's line weight follows `stroke`, in the design's own unit — the `strokeWidth` of the
//! 24-unit grid: `2` is the icon set's own weight, a menu's check is `2.6` (design artifacts:
//! Menu). Without it an icon keeps the set's weight; a weight outside 1–4 is clamped.

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout_types::{FxPx, ShapeKind};

const SRC: &str = r#"Page Main {
    Stack {
        Icon { symbol: "checkmark", size: 14 }
        Icon { symbol: "checkmark", size: 14, stroke: 2.6 }
        Icon { symbol: "checkmark", size: 14, stroke: 3 }
        Icon { symbol: "checkmark", size: 14, stroke: 9 }
    }
}
"#;

/// The stroke widths of the stroked glyphs, top to bottom (normalized `0..1000` viewbox).
fn strokes() -> Vec<u16> {
    let file = nexus_dsl_core::parse_file(SRC).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    let nxir = nexus_dsl_core::lower_file(&file, &model, SRC).expect("lowers").nxir;
    let symbols: Vec<String> =
        nexus_dsl_runtime::Runtime::mount(&nxir).expect("mounts").symbols().to_vec();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &nexus_theme_tokens::BaseTokens, &FixtureEnv::default(), &locale)
        .expect("mounts");
    let boxes = nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            FxPx::new(400),
            Some(FxPx::new(300)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes;
    let mut glyphs: Vec<(i32, u16)> = boxes
        .iter()
        .filter_map(|b| match &b.visual.shape {
            ShapeKind::Stroke { width_milli, .. } => Some((b.rect.y.0, *width_milli)),
            _ => None,
        })
        .collect();
    glyphs.sort();
    glyphs.into_iter().map(|(_, w)| w).collect()
}

#[test]
fn an_icon_takes_the_stroke_it_names_and_the_sets_own_without_one() {
    // Tenths of the 24-unit grid → the 0..1000 viewbox: 20 → 83, 26 → 108, 30 → 125, 40 → 167.
    assert_eq!(strokes(), vec![83, 108, 125, 167], "set's own · 2.6 · 3 · 9 clamped to 4");
}
