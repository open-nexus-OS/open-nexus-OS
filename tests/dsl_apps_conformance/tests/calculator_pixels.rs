// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// PIXELS, not geometry. Every other calculator test asserts layout boxes and
// scene text — all of which stay green while the screen renders nothing. This
// one reproduces the app-host glyph pass (`probe/paint.rs`): resolve each text
// run's face the way the painter does — `LayoutBox::text_px` overriding the
// authored size, re-resolved with the REQUESTED weight — rasterise it, and
// assert there is actually ink where the display is.
//
// …and then the step AFTER the glyphs, which is the one that actually blanked
// the calculator: the painter multiplies every fill and every glyph alpha by
// the node's animation opacity LAST. Rasterising ink proves nothing if a
// `.animate` resting at opacity 0 erases it a line later, so
// `nothing_rests_invisible_over_the_display` closes the ladder.

mod common;

use nexus_dsl_runtime::{AnimKind, FixtureEnv, IdentityLocale, Value, View};
use nexus_text_baked::measure_text::BakedTextMeasure;

struct NoHost;
impl nexus_dsl_runtime::EffectHost for NoHost {
    fn call(&mut self, _s: &str, _m: &str, _a: &[Value], _t: u32) -> Result<Value, u32> {
        Err(0)
    }
}

/// `(node index, content, authored face, requested weight)` — the same
/// pre-order walk `probe/paint/collect.rs` does.
fn collect(
    node: &nexus_layout_types::LayoutNode,
    index: &mut usize,
    out: &mut Vec<(usize, String, nexus_text_baked::FontSize, nexus_layout_types::FontWeight)>,
) {
    use nexus_layout_types::LayoutNode as N;
    *index += 1;
    match node {
        N::Text(t, _) => out.push((
            *index,
            String::from(t.content.as_str()),
            BakedTextMeasure::font(&t.style),
            t.style.font_weight,
        )),
        N::Stack(_, _, c) | N::Grid(_, _, c) => {
            for ch in c {
                collect(ch, index, out);
            }
        }
        _ => {}
    }
}

/// Total glyph coverage of one run, drawn exactly as the painter draws it.
fn ink_of(content: &str, font: nexus_text_baked::FontSize, width: i32) -> u32 {
    let w = width.max(1);
    let mut sum = 0u32;
    for y in 0..nexus_text_baked::line_height(font) {
        let mut row = vec![0u8; (w * 4) as usize];
        nexus_text_baked::draw_text_row(
            &mut row,
            y,
            0,
            0,
            w as u32,
            content.chars(),
            font,
            [255u8, 255, 255, 255],
        );
        sum += row.chunks_exact(4).map(|p| u32::from(p[3])).sum::<u32>();
    }
    sum
}

/// The display numeral must RENDER — at the handoff size and at the device's
/// own freeform window, where the fitted face is one of the new rungs.
#[test]
fn the_display_numeral_has_ink() {
    let nxir = common::compile("calculator");
    let symbols = common::program_symbols(&nxir);
    let keys = common::program_i18n_keys(&nxir);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;

    for (w, h) in [(392, 616), (960, 640), (1280, 800)] {
        let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
        view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
        let (e, c) = view.runtime().event_case("CalcEvent", "Digit").expect("Digit");
        for d in [1i64, 2, 3] {
            view.dispatch(
                &tokens,
                &device,
                &locale,
                &mut host,
                e,
                c,
                vec![Value::Fx(d << 32), Value::Str(d.to_string())],
            )
            .expect("digit");
        }
        let layout = nexus_layout::LayoutEngine::new()
            .layout_with_viewport(
                view.scene(),
                nexus_layout_types::FxPx::new(w),
                Some(nexus_layout_types::FxPx::new(h)),
                &BakedTextMeasure,
            )
            .expect("lays out");
        let mut runs = Vec::new();
        collect(view.scene(), &mut 0, &mut runs);

        let run = runs
            .iter()
            .find(|(_, content, _, _)| content == "123")
            .unwrap_or_else(|| panic!("{w}x{h}: the display run is missing: {runs:?}"));
        let b = layout
            .boxes
            .iter()
            .find(|b| b.node_id == run.0)
            .unwrap_or_else(|| panic!("{w}x{h}: no box for the display run"));

        // The painter's own resolution, verbatim.
        let font = match b.text_px {
            Some(px) => nexus_text_baked::FontSize::nearest(px.0, BakedTextMeasure::weight(run.3)),
            None => run.2,
        };
        assert!(
            b.rect.width.0 > 0 && b.rect.height.0 > 0,
            "{w}x{h}: the display box is empty: {:?}",
            b.rect
        );
        let ink = ink_of(&run.1, font, b.rect.width.0);
        assert!(
            ink > 0,
            "{w}x{h}: the display renders NO PIXELS (font {font:?}, box {}x{}, text_px {:?})",
            b.rect.width.0,
            b.rect.height.0,
            b.text_px.map(|p| p.0)
        );
    }
}

/// STAGE 7 — the animation fold. The stage every other calculator test skips,
/// and the one that blanked the display for a day.
///
/// `probe/paint.rs` scales each box fill (`paint_anim_box_row`) and each
/// glyph's alpha (`c[3] * a.opacity / 255`) by the node's `NodeAnim` LAST —
/// after the store, the scene, the layout and the raster are all provably
/// correct. `.animate(token, value:)` is a PRESENT/ABSENT binding, so a node
/// whose driving value folds to 0 rests at opacity 0
/// (`MotionToken::resting`), and `expand_node_anims` cascades that transform
/// to every box CONTAINED in the animated one. One modifier on a card
/// therefore erases the card, the history line and the number together, while
/// six green stages keep insisting the value is right.
///
/// Asserted as an invariant of the whole app rather than of one modifier: no
/// node may rest invisible over the display, whatever it is animating.
#[test]
fn nothing_rests_invisible_over_the_display() {
    let nxir = common::compile("calculator");
    let symbols = common::program_symbols(&nxir);
    let keys = common::program_i18n_keys(&nxir);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::desktop();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    let layout = nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(960),
            Some(nexus_layout_types::FxPx::new(640)),
            &BakedTextMeasure,
        )
        .expect("lays out");

    // The display run at rest is the store's initial `disp`.
    let mut runs = Vec::new();
    collect(view.scene(), &mut 0, &mut runs);
    let display = runs.iter().find(|(_, c, _, _)| c == "0").expect("the display run");
    let dbox = layout.boxes.iter().find(|b| b.node_id == display.0).expect("display box");

    for &(node_id, intent) in view.animations() {
        // Only `.animate` seeds a RESTING transform. `.transition` enters at
        // present, `.effect` seeds nothing, `Loop` is widget-internal.
        if intent.kind != AnimKind::Animate {
            continue;
        }
        let token = animation::MotionToken::from_id(intent.token).expect("a curated token");
        let prop = token.primary_prop();
        if prop != animation::AnimProp::Opacity || token.resting(prop, intent.value != 0) > 0.0 {
            continue;
        }
        let Some(b) = layout.boxes.iter().find(|b| b.node_id == node_id) else { continue };
        // The host's own cascade rule (`expand_node_anims`): a higher-id box
        // fully inside the animated one inherits its transform.
        let hides_display = node_id == dbox.node_id
            || (dbox.node_id > node_id
                && dbox.rect.x.0 >= b.rect.x.0
                && dbox.rect.y.0 >= b.rect.y.0
                && dbox.rect.x.0 + dbox.rect.width.0 <= b.rect.x.0 + b.rect.width.0
                && dbox.rect.y.0 + dbox.rect.height.0 <= b.rect.y.0 + b.rect.height.0);
        assert!(
            !hides_display,
            "node {node_id} rests at OPACITY 0 and contains the display box \
             ({:?} contains {:?}): `.animate({}, value: …)` folded its driving \
             value to {} — everything painted inside it is invisible on device, \
             however correct the text runs are.",
            b.rect,
            dbox.rect,
            token.name(),
            intent.value,
        );
    }
}

/// The same for a key label — the other half of the surface.
#[test]
fn key_labels_have_ink() {
    let nxir = common::compile("calculator");
    let symbols = common::program_symbols(&nxir);
    let keys = common::program_i18n_keys(&nxir);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::desktop();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    let layout = nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(960),
            Some(nexus_layout_types::FxPx::new(640)),
            &BakedTextMeasure,
        )
        .expect("lays out");
    let mut runs = Vec::new();
    collect(view.scene(), &mut 0, &mut runs);
    for label in ["7", "AC", "÷", "="] {
        let run = runs.iter().find(|(_, c, _, _)| c == label).expect("label present");
        let b = layout.boxes.iter().find(|b| b.node_id == run.0).expect("label box");
        let font = match b.text_px {
            Some(px) => nexus_text_baked::FontSize::nearest(px.0, BakedTextMeasure::weight(run.3)),
            None => run.2,
        };
        assert!(
            ink_of(&run.1, font, b.rect.width.0.max(1)) > 0,
            "key '{label}' renders no pixels (font {font:?}, text_px {:?})",
            b.text_px.map(|p| p.0)
        );
    }
}
