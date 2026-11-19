// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! `registry::motion_parts` against the builders (TASK-0354 P10): every part path a kit widget
//! stamps lands on the part it names — the toggle's on layer and thumb, the checkbox's on layer
//! and mark, the select's chevron, the slider's and the progress bar's fill. A builder whose
//! structure moved fails here, not on the board as a motion on the wrong box.

use nexus_dsl_runtime::{AnimKind, FixtureEnv, IdentityLocale, View};
use nexus_layout::LayoutBox;
use nexus_layout_types::ShapeKind;

fn mount(widget: &str) -> View<'static> {
    let src = format!(
        "Page Main {{\n    Stack {{\n        {widget}\n            .label(\"x\")\n    }}\n}}\n"
    );
    let file = nexus_dsl_core::parse_file(&src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    let nxir = nexus_dsl_core::lower_file(&file, &model, &src).expect("lowers").nxir;
    let nxir: &'static [u8] = Box::leak(nxir.into_boxed_slice());
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    View::mount(nxir, &nexus_theme_tokens::BaseTokens, &FixtureEnv::default(), &locale)
        .expect("mounts")
}

fn boxes(view: &View) -> Vec<LayoutBox> {
    nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(400),
            Some(nexus_layout_types::FxPx::new(300)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes
}

/// The boxes the widget's parts land on, by kind, in stamp order.
fn parts(widget: &str, kind: AnimKind) -> (Vec<LayoutBox>, Vec<LayoutBox>) {
    let view = mount(widget);
    let all = boxes(&view);
    let at = |id: usize| all.iter().find(|b| b.node_id == id).cloned().expect("a box per part");
    let picked = view.animations().iter().filter(|(_, i)| i.kind == kind).map(|(id, _)| at(*id));
    (picked.collect(), all.clone())
}

fn w(b: &LayoutBox) -> i32 {
    b.rect.width.0
}

#[test]
fn the_toggle_parts_are_its_on_layer_and_its_thumb() {
    let (layers, all) = parts("Toggle { checked: true }", AnimKind::Animate);
    let (thumbs, _) = parts("Toggle { checked: true }", AnimKind::Bound);
    let track = all.iter().find(|b| b.node_id + 1 == layers[0].node_id).expect("the track");
    assert_eq!(layers[0].rect, track.rect, "the on layer covers the track");
    assert!(layers[0].visual.shadow.is_some(), "the layer carries the glow");
    assert_eq!(w(&thumbs[0]), thumbs[0].rect.height.0, "the thumb is the round knob");
    assert!(w(&thumbs[0]) < w(track));
}

#[test]
fn the_checkbox_parts_are_its_on_layer_and_its_mark() {
    let (parts, _) = parts("Checkbox { checked: true }", AnimKind::Animate);
    assert_eq!(parts.len(), 2);
    assert!(parts[0].visual.shadow.is_some(), "the glowing on layer");
    assert!(matches!(parts[1].visual.shape, ShapeKind::Stroke { .. }), "the mark");
    let (off, _) = parts_off();
    assert!(off.iter().all(|b| b.opacity == 0), "unchecked: both parts rest invisible");
}

fn parts_off() -> (Vec<LayoutBox>, Vec<LayoutBox>) {
    parts("Checkbox { checked: false }", AnimKind::Animate)
}

#[test]
fn the_select_part_is_its_chevron() {
    let (closed, _) = parts("Select { value: \"a\", open: false }", AnimKind::Bound);
    let (open, _) = parts("Select { value: \"a\", open: true }", AnimKind::Bound);
    assert!(matches!(closed[0].visual.shape, ShapeKind::Stroke { .. }), "the chevron");
    assert_eq!((closed[0].visual.rotate_deg, open[0].visual.rotate_deg), (0, 180));
}

#[test]
fn the_slider_part_is_its_fill() {
    let (low, _) = parts("Slider { value: 20 }", AnimKind::Bound);
    let (high, _) = parts("Slider { value: 80 }", AnimKind::Bound);
    assert!(w(&low[0]) < w(&high[0]), "the fill grows with the value");
}

#[test]
fn the_progress_bar_part_is_its_fill() {
    for header in ["", ", label: \"Charge\""] {
        let (low, _) = parts(&format!("ProgressBar {{ value: 20{header} }}"), AnimKind::Bound);
        let (high, _) = parts(&format!("ProgressBar {{ value: 80{header} }}"), AnimKind::Bound);
        assert!(w(&low[0]) * 3 < w(&high[0]), "the fill is the value's share: {header}");
    }
    let (none, _) = parts("ProgressBar { }", AnimKind::Bound);
    assert!(none.is_empty(), "the indeterminate pip loops, it does not travel");
}
