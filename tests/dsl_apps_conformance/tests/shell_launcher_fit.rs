// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The launcher at the board's 1280×800 desktop (design artifacts: launcher): the full-screen
//! grid FITS the work band — 8 columns × 3 rows of xl icons, centred between the search row and
//! the page dots, the dots inside the bottom padding — and further pages sit BESIDE the first
//! (one strip node per page), not under it; the compact launcher is the 720×520 panel with a
//! four-column grid and its footer inside it; a single page draws no page dots and an empty list
//! draws no grid.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};

/// The registry: `n` apps for every `bundlemgr.enumerate`.
struct Registry {
    syms: Vec<(&'static str, u32)>,
    apps: usize,
}

impl nexus_dsl_runtime::EffectHost for Registry {
    fn call(&mut self, svc: &str, method: &str, _: &[Value]) -> Result<Value, u32> {
        if (svc, method) != ("bundlemgr", "enumerate") {
            return Ok(Value::List(Vec::new()));
        }
        let sym = |name: &str| self.syms.iter().find(|(n, _)| *n == name).expect(name).1;
        let rows = (0..self.apps)
            .map(|i| {
                let mut fields = vec![
                    (sym("id"), Value::Str(format!("app{i}"))),
                    (sym("label"), Value::Str(format!("App {i}"))),
                    (sym("icon"), Value::Str("star".into())),
                    (sym("iconTop"), Value::Str("#4ade80".into())),
                    (sym("iconBottom"), Value::Str("#15803d".into())),
                    (sym("iconArt"), Value::Str(String::new())),
                    (sym("running"), Value::Bool(false)),
                ];
                fields.sort_by_key(|(s, _)| *s);
                Value::Record(fields)
            })
            .collect();
        Ok(Value::List(rows))
    }
}

/// The desktop shell with `apps` registry entries and the launcher panel `panel` open.
fn shell(apps: usize, panel: &str) -> Vec<nexus_layout::LayoutBox> {
    let nxir = common::compile("desktop-shell");
    let symbols = common::program_symbols(&nxir);
    let keys = common::program_i18n_keys(&nxir);
    let syms = ["id", "label", "icon", "iconTop", "iconBottom", "iconArt", "running"]
        .iter()
        .map(|n| (*n, symbols.iter().position(|s| s == n).expect(n) as u32))
        .collect();
    let mut host = Registry { syms, apps };
    let device = FixtureEnv::desktop();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &nexus_theme_tokens::BaseTokens, &device, &locale)
        .expect("the shell mounts");
    view.run_initial_effects(&nexus_theme_tokens::BaseTokens, &device, &locale, &mut host)
        .expect("the initial effects run");
    common::dispatch_with_keys(
        &mut view,
        &device,
        &mut host,
        &symbols,
        &keys,
        "PanelEvent",
        "SetPanel",
        vec![Value::Str(panel.to_string())],
    );
    common::layout_boxes(&view)
}

fn sized(boxes: &[nexus_layout::LayoutBox], w: i32, h: i32) -> Vec<(i32, i32)> {
    boxes
        .iter()
        .filter(|b| b.rect.width.as_i32() == w && b.rect.height.as_i32() == h)
        .map(|b| (b.rect.x.as_i32(), b.rect.y.as_i32()))
        .collect()
}

fn distinct(mut v: Vec<i32>) -> Vec<i32> {
    v.sort_unstable();
    v.dedup();
    v
}

#[test]
fn the_full_screen_grid_fits_1280_by_800_and_pages_sit_side_by_side() {
    let boxes = shell(40, "launcher-big");
    let tiles = sized(&boxes, 88, 88);
    let first: Vec<_> = tiles.iter().filter(|(x, _)| *x < 1280).collect();
    let second: Vec<_> = tiles.iter().filter(|(x, _)| *x >= 1280).collect();
    assert_eq!(first.len(), 24, "8 × 3 icons on page one: {tiles:?}");
    assert_eq!(second.len(), 16, "the other 16 apps on page two: {tiles:?}");
    let cols = distinct(first.iter().map(|(x, _)| *x).collect());
    let rows = distinct(first.iter().map(|(_, y)| *y).collect());
    assert_eq!((cols.len(), rows.len()), (8, 3), "columns {cols:?} rows {rows:?}");
    // Page two sits beside page one: its rows are page one's rows, not rows below them.
    let rows_two = distinct(second.iter().map(|(_, y)| *y).collect());
    assert!(rows_two.iter().all(|y| rows.contains(y)), "page two under page one: {rows_two:?}");
    // The row pitch is the 136 px cell + the 20 px row gap.
    assert_eq!(rows[1] - rows[0], 156, "rows {rows:?}");
    // The search row (the 36 px round buttons) is at y 114 in the 36..744 band.
    let buttons = sized(&boxes, 36, 36);
    assert!(buttons.iter().any(|(_, y)| *y == 114), "search row at y 114: {buttons:?}");
    // The grid sits between the search row (ends 150) and the dots; the last row's label ends
    // above the active dot (20×8), which ends inside the bottom padding (band bottom 744 − 16).
    let top = rows[0] - 8;
    let bottom = rows[2] + 120 + 8;
    let dots = sized(&boxes, 20, 8);
    assert_eq!(dots.len(), 1, "one active page dot: {dots:?}");
    assert!(top >= 150 + 16, "the grid starts under the search row: {top}");
    assert!(bottom <= dots[0].1, "the grid ends above the page dots: {bottom} vs {dots:?}");
    assert!(dots[0].1 + 8 <= 744 - 16, "the dots end inside the band's bottom padding: {dots:?}");
    // Centred: the space above the rows equals the space below them, up to the 28 px control.
    let above = top - 166;
    let below = (dots[0].1 - 10) - bottom;
    assert!((above - below).abs() <= 8, "the rows are centred: above {above}, below {below}");
}

#[test]
fn a_single_page_has_no_page_dots_and_an_empty_list_no_grid() {
    let one = shell(10, "launcher-big");
    assert_eq!(sized(&one, 88, 88).len(), 10);
    assert!(sized(&one, 20, 8).is_empty() && sized(&one, 8, 8).is_empty(), "no dots");
    let none = shell(0, "launcher-big");
    assert!(sized(&none, 88, 88).is_empty(), "no grid without apps");
}

#[test]
fn the_compact_launcher_is_the_panel_with_a_four_column_grid_and_its_footer() {
    let boxes = shell(40, "launcher");
    let panels: Vec<_> = boxes
        .iter()
        .filter(|b| b.rect.width.as_i32() == 720 && b.rect.height.as_i32() == 520)
        .collect();
    assert!(!panels.is_empty(), "the 720×520 panel");
    let p = panels[0].rect;
    let (px, py) = (p.x.as_i32(), p.y.as_i32());
    assert_eq!(px, 280, "centred in 1280");
    assert_eq!(py + 520, 744 - 32, "32 px above the taskbar");
    let tiles: Vec<_> = sized(&boxes, 56, 56)
        .into_iter()
        .filter(|(x, y)| *x >= px && *x < px + 720 && *y >= py && *y < py + 520)
        .collect();
    let cols = distinct(tiles.iter().map(|(x, _)| *x).collect());
    assert_eq!(cols.len(), 4, "four columns: {cols:?}");
    // Head (36 + 16), section (14 + 12), the grid's 4 px, then the first tile row.
    let first_row = tiles.iter().map(|(_, y)| *y).min().unwrap();
    assert!(first_row >= py + 24 + 36 + 16 + 14 + 12 + 4, "the grid starts under the section row");
    // The footer's round buttons sit inside the panel, in its last 36 px.
    let footer: Vec<_> = sized(&boxes, 36, 36)
        .into_iter()
        .filter(|(x, y)| *x >= px && *x < px + 720 && *y >= py + 520 - 24 - 36 - 1)
        .collect();
    assert_eq!(
        distinct(footer.iter().map(|(x, _)| *x).collect()).len(),
        2,
        "settings and power: {footer:?}"
    );
    assert!(footer.iter().all(|(_, y)| *y + 36 <= py + 520 - 24 + 1), "inside the padding");
}
