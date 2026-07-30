// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! The REAL desktop-shell workspace grid (design_handoff_launcher §3.2):
//! compiles the shipped shell project, feeds it a 4-app registry, and pins
//! the md (56px) tiles onto distinct tracks of ONE grid row. Split out of
//! `layout_viewport.rs` (structure ratchet).

use nexus_dsl_runtime::{IdentityLocale, View};

fn symbols_of(nxir: &[u8]) -> Vec<String> {
    nexus_dsl_runtime::Runtime::mount(nxir).expect("mounts runtime").symbols().to_vec()
}

#[test]
fn shell_grid_tiles_lay_out_in_a_row() {
    struct Registry {
        id_sym: u32,
        label_sym: u32,
        icon_sym: u32,
        icon_top_sym: u32,
        icon_bottom_sym: u32,
        icon_art_sym: u32,
        running_sym: u32,
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
                let row = |id: &str, label: &str| {
                    let mut fields = vec![
                        (self.id_sym, Value::Str(id.into())),
                        (self.label_sym, Value::Str(label.into())),
                        (self.icon_sym, Value::Str("star".into())),
                        (self.icon_top_sym, Value::Str("#4ade80".into())),
                        (self.icon_bottom_sym, Value::Str("#15803d".into())),
                        (self.icon_art_sym, Value::Str("".into())),
                        // RFC-0086: the app-host merges the window feed's
                        // running/minimized/focused flags into every row —
                        // the shell tiles read `running`.
                        (self.running_sym, Value::Bool(false)),
                    ];
                    fields.sort_by_key(|(sym, _)| *sym);
                    Value::Record(fields)
                };
                return Ok(Value::List(vec![
                    row("a", "Alpha"),
                    row("b", "Beta"),
                    row("c", "Gamma"),
                    row("d", "Delta"),
                ]));
            }
            Err(0)
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop-shell");
    let nxir = nexus_dsl_core::compile_project_dir(&root).expect("compiles");
    let symbols = symbols_of(&nxir);
    let sym = |n: &str| symbols.iter().position(|s| s == n).expect(n) as u32;
    let mut host = Registry {
        id_sym: sym("id"),
        label_sym: sym("label"),
        icon_sym: sym("icon"),
        icon_top_sym: sym("iconTop"),
        icon_bottom_sym: sym("iconBottom"),
        icon_art_sym: sym("iconArt"),
        running_sym: sym("running"),
    };
    let device = nexus_dsl_runtime::FixtureEnv::tablet("landscape");
    let tokens = nexus_theme_tokens::BaseTokens;
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    let engine = nexus_layout::LayoutEngine::new();
    let boxes = engine
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(1280),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes;
    // The md (56px) tiles in the home-grid band (below topbar, above dock) —
    // design_handoff_launcher §3.2: the touch workspace is a real SIX-column
    // grid, so four tiles occupy four distinct tracks of one row.
    let tiles: Vec<(i32, i32)> = boxes
        .iter()
        .filter(|b| {
            b.rect.width.as_i32() == 56
                && b.rect.height.as_i32() == 56
                && b.rect.y.as_i32() > 40
                && b.rect.y.as_i32() < 700
        })
        .map(|b| (b.rect.x.as_i32(), b.rect.y.as_i32()))
        .collect();
    assert!(tiles.len() >= 4, "expected 4 grid tiles, got {tiles:?}");
    let first_y = tiles[0].1;
    assert!(tiles.iter().all(|(_, y)| *y == first_y), "grid tiles must share one row: {tiles:?}");
    let mut xs: Vec<i32> = tiles.iter().map(|(x, _)| *x).collect();
    xs.sort_unstable();
    xs.dedup();
    assert!(xs.len() >= 4, "four tiles on four distinct grid tracks: {tiles:?}");
}
