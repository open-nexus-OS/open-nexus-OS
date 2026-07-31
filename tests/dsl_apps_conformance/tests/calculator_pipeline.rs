// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// The calculator input→output pipeline, asserted STAGE BY STAGE, so a failure
// names the stage instead of just "the display is wrong":
//
//   1 hit-test   the tap lands on a node that owns a handler
//   2 dispatch   that handler reports visible damage
//   3 store      the reducer actually moved the state
//   4 scene      the view re-emitted the new value
//   5 layout     the value has a real box
//   6 pixels     the glyphs rasterise in that box
//   7 anim fold  nothing multiplies those pixels back to zero
//
// The device shows hover + press-grow on a key, which means stage 1 works
// there; everything below it is what these pin.
//
// Stage 7 lives in `calculator_pixels.rs`, and it is not decoration. This
// ladder once read green through stage 6 while the display was BLANK on
// device: `.animate(snappy, value: $state.disp)` folded a `Str` to 0, the host
// seeded the display card at opacity 0, and the painter — which applies the
// animation transform LAST — multiplied six correct stages away. A ladder that
// stops at "the glyphs rasterise" is a ladder that stops one rung below the
// bug.

mod common;

use nexus_dsl_runtime::{Damage, FixtureEnv, IdentityLocale, Value, View};
use nexus_text_baked::measure_text::BakedTextMeasure;

struct NoHost;
impl nexus_dsl_runtime::EffectHost for NoHost {
    fn call(&mut self, _s: &str, _m: &str, _a: &[Value], _t: u32) -> Result<Value, u32> {
        Err(0)
    }
}

fn texts(v: &View<'_>) -> Vec<String> {
    fn walk(n: &nexus_layout_types::LayoutNode, out: &mut Vec<String>) {
        match n {
            nexus_layout_types::LayoutNode::Stack(_, _, c)
            | nexus_layout_types::LayoutNode::Grid(_, _, c) => {
                for ch in c {
                    walk(ch, out);
                }
            }
            nexus_layout_types::LayoutNode::Text(t, _) => {
                out.push(String::from(t.content.as_str()))
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v.scene(), &mut out);
    out
}

#[test]
fn the_whole_pipeline_from_tap_to_pixels() {
    let nxir = common::compile("calculator");
    let symbols = common::program_symbols(&nxir);
    let keys = common::program_i18n_keys(&nxir);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::desktop();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");

    let (w, h) = (960, 640); // the device's freeform window
    let lay = |v: &View<'_>| {
        nexus_layout::LayoutEngine::new()
            .layout_with_viewport(
                v.scene(),
                nexus_layout_types::FxPx::new(w),
                Some(nexus_layout_types::FxPx::new(h)),
                &BakedTextMeasure,
            )
            .expect("lays out")
    };
    let boxes = lay(&view).boxes;

    // ---- stage 1: the tap lands on a handler-owning node -------------------
    let key_boxes: Vec<_> = view
        .handlers()
        .iter()
        .filter_map(|(id, _)| boxes.iter().find(|b| b.node_id == *id).map(|b| (*id, b)))
        .filter(|(_, b)| b.rect.y.0 > 130)
        .collect();
    assert_eq!(key_boxes.len(), 19, "stage 1: expected 19 key handlers");
    // Second keypad row, leftmost = "7".
    let row_y = {
        let mut ys: Vec<i32> = key_boxes.iter().map(|(_, b)| b.rect.y.0).collect();
        ys.sort_unstable();
        ys.dedup();
        ys[1]
    };
    let (seven_id, seven) = key_boxes
        .iter()
        .filter(|(_, b)| b.rect.y.0 == row_y)
        .min_by_key(|(_, b)| b.rect.x.0)
        .expect("stage 1: the 7 key");
    let (tx, ty) =
        (seven.rect.x.0 + seven.rect.width.0 / 2, seven.rect.y.0 + seven.rect.height.0 / 2);
    // Hit-testing takes the INNERMOST match (largest pre-order id), so the key
    // only wins if no later handler box also covers the point.
    let covering: Vec<usize> = view
        .handlers()
        .iter()
        .filter_map(|(id, _)| boxes.iter().find(|b| b.node_id == *id))
        .filter(|b| {
            b.rect.x.0 <= tx
                && b.rect.y.0 <= ty
                && b.rect.x.0 + b.rect.width.0 > tx
                && b.rect.y.0 + b.rect.height.0 > ty
        })
        .map(|b| b.node_id)
        .collect();
    assert_eq!(
        covering.iter().max().copied(),
        Some(*seven_id),
        "stage 1: the innermost handler under the tap must be the 7 key, not a \
         later-id sibling ({covering:?})"
    );

    let before_texts = texts(&view);

    // ---- stage 2: the handler dispatches with visible damage ---------------
    let damage = view
        .pointer_scrolled(
            &tokens,
            &device,
            &locale,
            &mut host,
            &boxes,
            "Tap",
            nexus_layout_types::FxPx::new(tx),
            nexus_layout_types::FxPx::new(ty),
            None,
        )
        .expect("stage 2: pointer path errored");
    assert!(
        matches!(damage, Some(Damage::Layout)),
        "stage 2: a digit changes text, which is LAYOUT-class damage; got {damage:?}. \
         `Some(Paint)` or `None` means a handler ran but the store did not move."
    );

    // ---- stage 3+4: the store moved and the view re-emitted ----------------
    let after_texts = texts(&view);
    assert_ne!(
        before_texts, after_texts,
        "stage 3/4: the scene is identical after the tap — the reducer wrote the \
         same value, or the display is not bound to what it wrote"
    );
    assert!(
        after_texts.iter().any(|t| t == "7"),
        "stage 4: the display must show 7, got {after_texts:?}"
    );

    // ---- stage 5: the value has a real box ---------------------------------
    let after = lay(&view);
    let display_box = after
        .boxes
        .iter()
        .find(|b| b.rect.y.0 < 130 && b.rect.width.0 > 0 && b.text_px.is_some())
        .expect("stage 5: the display numeral has no box");
    assert!(
        display_box.rect.height.0 > 0,
        "stage 5: the display box has zero height: {:?}",
        display_box.rect
    );

    // ---- stage 6: the glyphs rasterise -------------------------------------
    let font = nexus_text_baked::FontSize::nearest(
        display_box.text_px.expect("fitted").0,
        nexus_text_baked::Weight::Light,
    );
    let mut ink = 0u32;
    for row in 0..nexus_text_baked::line_height(font) {
        let mut buf = vec![0u8; (display_box.rect.width.0.max(1) * 4) as usize];
        nexus_text_baked::draw_text_row(
            &mut buf,
            row,
            0,
            0,
            display_box.rect.width.0.max(1) as u32,
            "7".chars(),
            font,
            [255, 255, 255, 255],
        );
        ink += buf.chunks_exact(4).map(|p| u32::from(p[3])).sum::<u32>();
    }
    assert!(ink > 0, "stage 6: the display face {font:?} rasterises nothing");
}
