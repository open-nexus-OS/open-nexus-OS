// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! An Int modifier argument may be an expression, as a px argument may (RFC-0095): a component
//! prop (`.columns($props.cols)`) or a store field (`.grow($state.share)`). The checker always
//! accepted the form; the runtime read it as 0 — one column, no growth — a silent no-op an app
//! author could not see. A store field is a LAYOUT dependency: the layout follows its changes.

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};
use nexus_layout_types::FxPx;

struct NoHost;
impl nexus_dsl_runtime::EffectHost for NoHost {
    fn call(&mut self, _s: &str, _m: &str, _a: &[Value]) -> Result<Value, u32> {
        Err(0)
    }
}

const SRC: &str = r#"Store S {
    share: Int = 1,
    items: List<Str> = ["a", "b", "c", "d", "e", "f", "g"],
}
Event E {
    Widen,
}
reduce E {
    Widen => state.share = 3,
}
Component Tiles {
    props: {
        cols: Int,
    }
    List($state.items) { item in
        Stack { Text(item) }
            .key(item)
            .height(40)
    }
    .columns($props.cols)
    .gap(0)
}
Page Main {
    Stack {
        Tiles { cols: 3 }
        Stack {
            Stack { }
                .height(24)
                .grow($state.share)
            Stack { }
                .height(24)
                .grow(1)
        }
        .direction(row)
    }
}
"#;

fn compile() -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(SRC).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    nexus_dsl_core::lower_file(&file, &model, SRC).expect("lowers").nxir
}

fn boxes(view: &View<'_>) -> Vec<nexus_layout::LayoutBox> {
    nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            FxPx::new(1280),
            Some(FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes
}

/// The two 24 px high boxes in the row (not the full-width row), left to right: (x, width).
fn row_widths(view: &View<'_>) -> Vec<(i32, i32)> {
    let mut row: Vec<(i32, i32)> = boxes(view)
        .iter()
        .filter(|b| b.rect.height.0 == 24 && b.rect.width.0 < 1280)
        .map(|b| (b.rect.x.0, b.rect.width.0))
        .collect();
    row.sort();
    row
}

#[test]
fn a_prop_sets_the_grid_columns() {
    let nxir = compile();
    let symbols: Vec<String> =
        nexus_dsl_runtime::Runtime::mount(&nxir).expect("mounts").symbols().to_vec();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &nexus_theme_tokens::BaseTokens, &FixtureEnv::default(), &locale)
        .expect("mounts");
    // The tiles are the 40 px high boxes: three to a row, seven tiles on three rows.
    let tiles: Vec<_> = boxes(&view).into_iter().filter(|b| b.rect.height.0 == 40).collect();
    assert_eq!(tiles.len(), 7, "seven tiles");
    let rows: std::collections::BTreeSet<i32> = tiles.iter().map(|b| b.rect.y.0).collect();
    assert_eq!(rows.len(), 3, "3 columns lay 7 tiles on 3 rows, not one column of 7: {rows:?}");
}

#[test]
fn a_store_field_sets_the_grow_and_the_layout_follows_it() {
    let nxir = compile();
    let symbols: Vec<String> =
        nexus_dsl_runtime::Runtime::mount(&nxir).expect("mounts").symbols().to_vec();
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let before = row_widths(&view);
    assert_eq!(before.len(), 2, "the row's two boxes: {before:?}");
    assert_eq!(before[0].1, before[1].1, "share 1 against 1: equal halves {before:?}");

    let (e, c) = view.runtime().event_case("E", "Widen").expect("Widen");
    view.dispatch(&tokens, &device, &locale, &mut NoHost, e, c, vec![]).expect("dispatch");
    let after = row_widths(&view);
    assert_eq!(after[0].1, 3 * after[1].1, "share 3 against 1: three quarters {after:?}");
}
