// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0083: a settings change is a REEMIT, never a remount.
//!
//! These proofs pin the runtime-level halves the app-host's
//! `apply_settings` (probe/presentation.rs) is built on, against the REAL
//! compiled shell:
//! - a token swap recolors the scene while every bit of store state survives
//!   (the legacy path tore the whole app down for an accent tap);
//! - a profile change re-selects page STRUCTURE via reemit (the size-class
//!   mechanism) while running ZERO service effects — the remount used to
//!   re-run initial effects and re-fetch session.users/bundlemgr.enumerate
//!   on every switch, which is exactly the measured freeze.

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};
use nexus_layout_types::Rgba8;
use nexus_theme_tokens::{AccentTokens, DarkTokens, Tokens};

/// Counts every `svc.*` effect call — the "no service storm" instrument.
struct CountingHost {
    calls: usize,
}
impl nexus_dsl_runtime::EffectHost for CountingHost {
    fn call(&mut self, _: &str, _: &str, _: &[Value], _: u32) -> Result<Value, u32> {
        self.calls += 1;
        Ok(Value::Bool(true))
    }
}

fn mount<'p>(env: &FixtureEnv, tokens: &dyn Tokens, nxir: &'p [u8]) -> (View<'p>, Vec<String>) {
    let symbols = common::program_symbols(nxir);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(nxir, tokens, env, &locale).expect("mounts");
    (view, symbols)
}

fn background_colors(view: &View, env_w: i32) -> Vec<[u8; 4]> {
    let boxes = nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(env_w),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes
        .iter()
        .filter_map(|b| b.visual.background)
        .map(|c| [c.r, c.g, c.b, c.a])
        .collect::<Vec<_>>();
    boxes
}

/// Accent tap = recolor, not teardown. Store state (an OPEN panel) must
/// survive the reemit bit-for-bit, and the scene must actually change color.
#[test]
fn a_token_swap_reemit_recolors_without_losing_state() {
    let nxir = common::compile("desktop-shell");
    let device = FixtureEnv::tablet("landscape");
    let base = nexus_theme_tokens::BaseTokens;
    let (mut view, symbols) = mount(&device, &base, &nxir);
    let mut host = CountingHost { calls: 0 };

    // Build store state the reemit must not lose: open the Control Center.
    common::dispatch(
        &mut view,
        &device,
        &mut host,
        &symbols,
        "PanelEvent",
        "SetPanel",
        vec![Value::Str("control".to_string())],
    );
    let panel_open = |v: &View| {
        common::layout_boxes(v)
            .iter()
            .any(|b| b.rect.width.0 >= 300 && b.rect.height.0 >= 300 && b.rect.y.0 > 36)
    };
    assert!(panel_open(&view), "precondition: the Control Center is open");
    let before = background_colors(&view, 1280);

    // The accent swap: reemit under a different token set — the exact call
    // apply_settings makes. No mount, no store snapshot, no effects.
    let accented = AccentTokens { base: &DarkTokens, accent: Rgba8::new(220, 38, 38, 255) };
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let calls_before = host.calls;
    view.reemit(&accented, &device, &locale).expect("reemit succeeds");

    assert!(panel_open(&view), "the open panel SURVIVED the re-theme (store intact)");
    let after = background_colors(&view, 1280);
    assert_ne!(before, after, "the scene actually changed color");
    assert_eq!(host.calls, calls_before, "a re-theme runs ZERO service effects");
}

/// Profile switch = structure re-selection via reemit (the size-class
/// mechanism), with ZERO service effects. The legacy remount re-ran initial
/// effects and re-fetched services on every desktop⇄tablet flip.
#[test]
fn a_profile_reemit_reselects_structure_without_running_effects() {
    let nxir = common::compile("desktop-shell");
    let base = nexus_theme_tokens::BaseTokens;
    let tablet = FixtureEnv::tablet("landscape");
    let desktop = FixtureEnv::desktop();
    let (mut view, symbols) = mount(&tablet, &base, &nxir);
    let mut host = CountingHost { calls: 0 };

    // Store state to carry across: open a panel in tablet mode.
    common::dispatch(
        &mut view,
        &tablet,
        &mut host,
        &symbols,
        "PanelEvent",
        "SetPanel",
        vec![Value::Str("control".to_string())],
    );
    let tablet_structure = structure(&view);

    // The reference: a FRESH desktop mount (the compiler wraps
    // `ui/platform/desktop` pages in `device.profile == "desktop"` arms —
    // project.rs merge) with the SAME store state (panel open), because the
    // reemit is expected to carry that state across the profile flip.
    let (mut fresh_desktop, fresh_symbols) = mount(&desktop, &base, &nxir);
    common::dispatch(
        &mut fresh_desktop,
        &desktop,
        &mut host,
        &fresh_symbols,
        "PanelEvent",
        "SetPanel",
        vec![Value::Str("control".to_string())],
    );
    let desktop_structure = structure(&fresh_desktop);
    assert_ne!(
        tablet_structure, desktop_structure,
        "precondition: the two profiles select different page structures"
    );

    // The profile flip: reemit under the DESKTOP env — apply_settings'
    // profile arm. Structure must change; no mount, no effects.
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let calls_before = host.calls;
    view.reemit(&base, &desktop, &locale).expect("reemit succeeds");

    assert_eq!(
        structure(&view),
        desktop_structure,
        "the reemit re-selected the desktop override page"
    );
    assert_eq!(host.calls, calls_before, "a profile switch runs ZERO service effects");

    // And back — idempotent round trip, still no effects, same store: the
    // panel opened above is tablet-page store state and must still be there.
    view.reemit(&base, &tablet, &locale).expect("reemit back succeeds");
    assert_eq!(structure(&view), tablet_structure, "round trip restores the structure");
    assert_eq!(host.calls, calls_before);
}

/// A structural fingerprint: every box's rect (position + size). Two
/// different page trees cannot collide on this at 1280x800; text content is
/// deliberately excluded (i18n keys resolve empty under IdentityLocale).
fn structure(view: &View) -> Vec<(i32, i32, i32, i32)> {
    common::layout_boxes(view)
        .iter()
        .map(|b| (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0))
        .collect()
}
