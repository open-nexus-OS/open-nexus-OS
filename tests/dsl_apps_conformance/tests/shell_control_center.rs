// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Control Center panel absorbs every tap that lands on it.
//!
//! Written to settle a false alarm. Interactive boot logs reported
//! `apphost: input tap miss` at the SAME three coordinates run after run —
//! including boots that predate every change blamed for it. The panel is
//! wrapped in a full-bleed `.overlay()` scrim carrying `SetPanel("")`, so a tap
//! there cannot legitimately reach nothing, and this test says so against the
//! REAL compiled shell rather than against a reading of the log.
//!
//! What those taps actually hit: the panel wrapper's deliberate `PanelNoop`
//! absorber and a live 156x48 control. Neither repaints, and `tap()` used to
//! collapse "no repaint" and "no handler" into one bool — see
//! app-host `probe::interaction::TapOutcome`.

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};
use nexus_layout_types::FxPx;

/// The exact points the boot logs kept reporting as misses.
const REPORTED_AS_MISSES: &[(i32, i32)] = &[(1152, 180), (1129, 187), (1237, 237), (1235, 236)];

struct NullHost;
impl nexus_dsl_runtime::EffectHost for NullHost {
    fn call(&mut self, _: &str, _: &str, _: &[Value]) -> Result<Value, u32> {
        Ok(Value::Bool(true))
    }
}

#[test]
fn every_tap_on_the_open_control_center_reaches_a_handler() {
    let nxir = common::compile("desktop-shell");
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let symbols = common::program_symbols(&nxir);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = NullHost;

    // Open the Control Center exactly the way the top-bar pill does.
    common::dispatch(
        &mut view,
        &device,
        &mut host,
        &symbols,
        "PanelEvent",
        "SetPanel",
        vec![Value::Str("control".to_string())],
    );
    let boxes = common::layout_boxes(&view);

    let unhandled: Vec<_> = REPORTED_AS_MISSES
        .iter()
        .filter(|&&(x, y)| {
            view.hover_box_id_scrolled(&boxes, "Tap", FxPx::new(x), FxPx::new(y), None).is_none()
        })
        .collect();
    assert!(
        unhandled.is_empty(),
        "no handler at {unhandled:?} — the full-bleed scrim alone must catch every point"
    );

    // The user-facing invariant: no DEAD ZONE inside the panel itself. Every
    // point of the Control Center body must reach something — a control, or
    // the wrapper's `PanelNoop` absorber that keeps a near-miss from closing
    // the panel out from under you.
    let panel = boxes
        .iter()
        .filter(|b| b.rect.width.0 >= 300 && b.rect.height.0 >= 300 && b.rect.y.0 > 36)
        .min_by_key(|b| b.rect.width.0 * b.rect.height.0)
        .expect("the open Control Center has a panel box");
    let (x0, y0) = (panel.rect.x.0, panel.rect.y.0);
    let (x1, y1) = (x0 + panel.rect.width.0, y0 + panel.rect.height.0);
    for y in (y0..y1).step_by(17) {
        for x in (x0..x1).step_by(19) {
            assert!(
                view.hover_box_id_scrolled(&boxes, "Tap", FxPx::new(x), FxPx::new(y), None)
                    .is_some(),
                "({x},{y}) is a dead zone inside the Control Center panel \
                 ({x0},{y0} {}x{})",
                panel.rect.width.0,
                panel.rect.height.0
            );
        }
    }
}

/// TASK-0077B P2b — the Control Center's volume slider against the REAL
/// compiled shell: a tap along the track writes the value.
///
/// Until this package the slider was a readout. `CcBrightnessCard.nx` said so
/// in a comment and predicted that the value-carrying interaction would land
/// without touching the markup — it did: the app declares nothing but the
/// binding, no `on Tap`, no payload.
///
/// The second half is the invariant that made the volume slider risky to bring
/// to life: `muted` used to be a Bool the `SetVolume` reducer kept in step, and
/// a bind writes the FIELD, not the reducer — so a live slider would have moved
/// the volume while the mute button and the top bar kept the old answer. Muted
/// is now `volume == 0`, one fact, and this pins that there is no second copy
/// left to disagree.
#[test]
fn tapping_the_volume_slider_writes_the_value_and_mute_cannot_disagree() {
    let nxir = common::compile("desktop-shell");
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let symbols = common::program_symbols(&nxir);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = NullHost;

    assert_eq!(
        view.runtime.field("ControlStore", "muted"),
        None,
        "`muted` must not exist as a second copy of `volume == 0`"
    );

    common::dispatch(
        &mut view,
        &device,
        &mut host,
        &symbols,
        "PanelEvent",
        "SetPanel",
        vec![Value::Str("control".to_string())],
    );

    // The slider bound to `volume`, found by its WRITE TARGET — no widget kind
    // and no coordinate guessing.
    let volume_sym = symbols.iter().position(|s| s == "volume").expect("the store names `volume`");
    let slider_box = view
        .handlers()
        .iter()
        .find_map(|(box_id, h)| match &h.action {
            nexus_dsl_runtime::interact::HandlerAction::Bind { path, .. } => {
                (path.first() == Some(&(volume_sym as u32))).then_some(*box_id)
            }
            _ => None,
        })
        .expect("the Control Center's volume slider auto-binds");

    let mut tap_at = |numerator: i32, denominator: i32| {
        let boxes = common::layout_boxes(&view);
        let rect = boxes.iter().find(|b| b.node_id == slider_box).expect("slider box").rect;
        assert!(rect.width.0 > 1, "the track must have travel to measure along");
        view.pointer(
            &tokens,
            &device,
            &locale,
            &mut host,
            &boxes,
            "Tap",
            FxPx::new(rect.x.0 + (rect.width.0 - 1) * numerator / denominator),
            FxPx::new(rect.y.0 + rect.height.0 / 2),
        )
        .expect("routes");
        match view.runtime.field("ControlStore", "volume") {
            Some(Value::Int(v)) => *v,
            other => panic!("volume is not an Int: {other:?}"),
        }
    };

    assert_eq!(tap_at(3, 4), 75, "three quarters along the real track");
    assert_eq!(tap_at(0, 1), 0, "the first pixel silences it");
    assert_eq!(tap_at(1, 1), 100, "the last pixel is full volume");
}
