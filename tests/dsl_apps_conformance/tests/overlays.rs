// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0074 conformance: modal semantics on the app-owned `.overlay()` primitive, against
//! the REAL runtime (compile → mount → layout → pointer/ESC/timeout):
//!
//! - the bounded stack — the fifth modal is refused at emit;
//! - confinement — while a modal is open a tap on a control BEHIND it (outside the modal's
//!   panel, under the layer's backdrop) reaches nothing — the layer's backdrop rule fires
//!   `on Dismiss` instead; a text field behind it cannot take focus; hover resolves nothing
//!   outside the modal's subtree;
//! - the absorbing variant — a layer that takes its own taps (`on Tap -> Noop`, the alert
//!   pattern) neither leaks nor dismisses;
//! - ESC — `dismiss_top` fires the TOPMOST modal's handler (inner before outer) and does
//!   nothing with no modal open;
//! - transient — the declared `.dismissAfter` is readable, the timeout path fires the
//!   handler with reason Timeout.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use nexus_dsl_runtime::interact::HandlerAction;
use nexus_dsl_runtime::{
    DismissReason, FixtureEnv, IdentityLocale, OverlayKind, RtError, Value, View,
};
use nexus_layout_types::FxPx;
use nexus_theme_tokens::BaseTokens;

const PROGRAM: &str = r#"
Store S {
    count: Int = 0,
    modal: Bool = false,
    inner: Bool = false,
    toast: Bool = true,
    name: Str = "",
    inname: Str = "",
}

Event E {
    Bg,
    ModalBtn,
    OpenModal,
    CloseModal,
    OpenInner,
    CloseInner,
    HideToast,
    Noop,
}

reduce E {
    Bg => state.count = state.count + 1,
    ModalBtn => state.count = state.count + 10,
    OpenModal => state.modal = true,
    CloseModal => state.modal = false,
    OpenInner => state.inner = true,
    CloseInner => state.inner = false,
    HideToast => state.toast = false,
    Noop => state.count = state.count,
}

Page P {
    Stack {
        Button { label: "bg" } on Tap -> dispatch(Bg)
        TextField { value: $state.name }.label("name")
        if $state.modal {
            Stack {
                Stack { }
                    .height(400)
                Stack {
                    Button { label: "in" } on Tap -> dispatch(ModalBtn)
                    TextField { value: $state.inname }.label("inner name")
                    Button { label: "open inner" } on Tap -> dispatch(OpenInner)
                    if $state.inner {
                        Stack {
                            Button { label: "inner" } on Tap -> dispatch(Noop)
                        }
                        .overlay(modal)
                        on Tap -> dispatch(Noop)
                        on Dismiss -> dispatch(CloseInner)
                    } else {
                        Stack { }
                    }
                }
                .width(240)
                .height(240)
                on Tap -> dispatch(Noop)
            }
            .overlay(modal)
            on Dismiss -> dispatch(CloseModal)
        } else {
            Stack { }
        }
        if $state.toast {
            Stack {
                Text("toast")
            }
            .overlay(transient)
            .dismissAfter(3000)
            on Dismiss -> dispatch(HideToast)
        } else {
            Stack { }
        }
    }
}
"#;

fn compile(source: &str) -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(source).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check errors: {diags:?}");
    let canonical = nexus_dsl_core::format_file(&file);
    nexus_dsl_core::lower_file(&file, &model, &canonical).expect("lowers").nxir
}

struct NullHost;
impl nexus_dsl_runtime::EffectHost for NullHost {
    fn call(&mut self, _: &str, _: &str, _: &[Value]) -> Result<Value, u32> {
        Ok(Value::Bool(true))
    }
}

struct T<'p> {
    view: View<'p>,
    symbols: Vec<String>,
    device: FixtureEnv,
}

impl<'p> T<'p> {
    fn mount(nxir: &'p [u8]) -> Self {
        let symbols = common::program_symbols(nxir);
        let device = FixtureEnv::desktop();
        let view = {
            let keys: Vec<u32> = Vec::new();
            let locale = IdentityLocale { symbols: &symbols, keys: &keys };
            View::mount(nxir, &BaseTokens, &device, &locale).expect("mounts")
        };
        Self { view, symbols, device }
    }

    fn field(&self, name: &str) -> Value {
        self.view.runtime.field("S", name).cloned().unwrap_or_else(|| panic!("field {name}"))
    }

    fn dispatch(&mut self, case: &str) {
        let mut host = NullHost;
        common::dispatch(&mut self.view, &self.device, &mut host, &self.symbols, "E", case, vec![]);
    }

    /// Center of the FIRST Tap handler that dispatches `E::case`.
    fn center(&self, case: &str) -> (i32, i32) {
        let boxes = common::layout_boxes(&self.view);
        let (e, c) = self.view.runtime.event_case("E", case).expect("case");
        let tap = self.symbols.iter().position(|s| s == "Tap").expect("Tap") as u32;
        let (box_id, _) = self
            .view
            .handlers()
            .iter()
            .find(|(_, h)| {
                h.trigger == tap
                    && matches!(&h.action, HandlerAction::Dispatch { event, case, .. } if *event == e && *case == c)
            })
            .unwrap_or_else(|| panic!("no Tap handler dispatching {case}"));
        let b = boxes.iter().find(|b| b.node_id == *box_id).expect("box");
        (b.rect.x.0 + b.rect.width.0 / 2, b.rect.y.0 + b.rect.height.0 / 2)
    }

    fn tap(&mut self, (x, y): (i32, i32)) -> Option<nexus_dsl_runtime::Damage> {
        let boxes = common::layout_boxes(&self.view);
        let keys: Vec<u32> = Vec::new();
        let locale = IdentityLocale { symbols: &self.symbols, keys: &keys };
        let mut host = NullHost;
        self.view
            .pointer_scrolled(
                &BaseTokens,
                &self.device,
                &locale,
                &mut host,
                &boxes,
                "Tap",
                FxPx::new(x),
                FxPx::new(y),
                None,
            )
            .expect("tap runs")
    }

    fn hover(&self, (x, y): (i32, i32)) -> Option<usize> {
        let boxes = common::layout_boxes(&self.view);
        self.view.hover_box_id_scrolled(&boxes, "Tap", FxPx::new(x), FxPx::new(y), None)
    }

    fn escape(&mut self) -> Option<nexus_dsl_runtime::Damage> {
        let keys: Vec<u32> = Vec::new();
        let locale = IdentityLocale { symbols: &self.symbols, keys: &keys };
        let mut host = NullHost;
        self.view
            .dismiss_top(&BaseTokens, &self.device, &locale, &mut host, DismissReason::Escape)
            .expect("escape runs")
    }

    fn focus_at(&mut self, (x, y): (i32, i32)) -> bool {
        let boxes = common::layout_boxes(&self.view);
        self.view.focus_text_at(&boxes, FxPx::new(x), FxPx::new(y), None).is_some()
    }

    /// Center of the n-th text field (a `Change` bind handler), in handler order.
    fn field_center(&self, nth: usize) -> (i32, i32) {
        let boxes = common::layout_boxes(&self.view);
        let change = self.symbols.iter().position(|s| s == "Change").expect("Change") as u32;
        let (box_id, _) = self
            .view
            .handlers()
            .iter()
            .filter(|(_, h)| h.trigger == change && matches!(h.action, HandlerAction::Bind { .. }))
            .nth(nth)
            .expect("text field");
        let b = boxes.iter().find(|b| b.node_id == *box_id).expect("box");
        (b.rect.x.0 + b.rect.width.0 / 2, b.rect.y.0 + b.rect.height.0 / 2)
    }
}

#[test]
fn test_reject_the_fifth_modal_at_mount() {
    let layer =
        |inner: &str| format!("Stack {{ {inner} }}.overlay(modal) on Dismiss -> dispatch(Noop)");
    let mut body = String::from("Text(\"deep\")");
    for _ in 0..5 {
        body = layer(&body);
    }
    let src = format!(
        "Store S {{ x: Int = 0, }}\nEvent E {{ Noop, }}\nreduce E {{ Noop => state.x = state.x, }}\nPage P {{ Stack {{ {body} }} }}"
    );
    let nxir = compile(&src);
    let symbols = common::program_symbols(&nxir);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let err = View::mount(&nxir, &BaseTokens, &FixtureEnv::desktop(), &locale).err();
    assert!(
        matches!(err, Some(nexus_dsl_runtime::MountError::Rt(RtError::OverlayDepth))),
        "the fifth modal is refused: {err:?}"
    );

    // Four nest fine.
    let mut body = String::from("Text(\"deep\")");
    for _ in 0..4 {
        body = layer(&body);
    }
    let src = format!(
        "Store S {{ x: Int = 0, }}\nEvent E {{ Noop, }}\nreduce E {{ Noop => state.x = state.x, }}\nPage P {{ Stack {{ {body} }} }}"
    );
    let nxir = compile(&src);
    let symbols = common::program_symbols(&nxir);
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view =
        View::mount(&nxir, &BaseTokens, &FixtureEnv::desktop(), &locale).expect("four mount");
    assert_eq!(view.overlays().modal_depth(), 4);
}

#[test]
fn a_modal_confines_taps_focus_and_hover_and_the_backdrop_dismisses() {
    let nxir = compile(PROGRAM);
    let mut t = T::mount(&nxir);
    assert_eq!(t.view.overlays().modal_depth(), 0);
    assert!(t.view.modal_confine().is_none());
    let bg = t.center("Bg");
    let outer_field = t.field_center(0);

    // Without a modal: the background button and field work.
    t.tap(bg);
    assert_eq!(t.field("count"), Value::Int(1));
    assert!(t.focus_at(outer_field));
    assert!(t.hover(bg).is_some());

    t.dispatch("OpenModal");
    assert_eq!(t.view.overlays().modal_depth(), 1);
    assert!(t.view.modal_confine().is_some());

    // The background button lies BEHIND the layer: its tap reaches nothing —
    // the layer's backdrop rule fires `on Dismiss` instead (reason Backdrop).
    assert!(t.hover(bg).is_none(), "hover resolves nothing outside the modal");
    assert!(!t.focus_at(outer_field), "a field behind the modal cannot take focus");
    assert_eq!(t.field("count"), Value::Int(1));
    t.tap(bg);
    assert_eq!(t.field("count"), Value::Int(1), "background tap never reached the button");
    assert_eq!(t.view.take_dismissed(), Some(DismissReason::Backdrop));
    assert_eq!(t.field("modal"), Value::Bool(false));
    assert_eq!(t.view.overlays().modal_depth(), 0);

    // Inside the modal, controls and fields work; the panel absorbs.
    t.dispatch("OpenModal");
    let inside = t.center("ModalBtn");
    let inner_field = t.field_center(1);
    assert!(t.hover(inside).is_some());
    t.tap(inside);
    assert_eq!(t.field("count"), Value::Int(11));
    assert!(t.focus_at(inner_field));
    assert_eq!(t.view.take_dismissed(), None, "a handled tap is no dismissal");
    assert_eq!(t.field("modal"), Value::Bool(true));
}

#[test]
fn nested_modals_escape_innermost_first_and_an_absorbing_layer_leaks_nothing() {
    let nxir = compile(PROGRAM);
    let mut t = T::mount(&nxir);
    assert_eq!(t.escape(), None, "ESC with no modal open changes nothing");
    assert_eq!(t.view.take_dismissed(), None);

    t.dispatch("OpenModal");
    t.dispatch("OpenInner");
    assert_eq!(t.view.overlays().modal_depth(), 2);
    let outer_btn = t.center("ModalBtn");
    // The inner layer absorbs (`on Tap -> Noop`): the outer modal's button
    // below it neither fires nor dismisses anything.
    t.tap(outer_btn);
    assert_eq!(t.field("count"), Value::Int(0));
    assert_eq!(t.view.take_dismissed(), None);
    assert_eq!(t.field("inner"), Value::Bool(true));

    // ESC closes the INNER modal first, then the outer one.
    t.escape();
    assert_eq!(t.view.take_dismissed(), Some(DismissReason::Escape));
    assert_eq!(t.field("inner"), Value::Bool(false));
    assert_eq!(t.field("modal"), Value::Bool(true));
    assert_eq!(t.view.overlays().modal_depth(), 1);
    t.escape();
    assert_eq!(t.view.take_dismissed(), Some(DismissReason::Escape));
    assert_eq!(t.field("modal"), Value::Bool(false));
    assert_eq!(t.view.overlays().modal_depth(), 0);
}

#[test]
fn a_transient_declares_its_timeout_and_leaves_through_its_handler() {
    let nxir = compile(PROGRAM);
    let mut t = T::mount(&nxir);
    let toast = t.view.overlays().transients().next().cloned().expect("the toast is up");
    assert_eq!(toast.kind, OverlayKind::Transient);
    assert_eq!(toast.dismiss_after_ms, Some(3000));
    assert_eq!(t.view.overlays().modal_depth(), 0, "a transient confines nothing");
    // The host's timer fires: the handler runs with reason Timeout.
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &t.symbols, keys: &keys };
    let mut host = NullHost;
    let damage = t
        .view
        .dismiss_at(&BaseTokens, &t.device, &locale, &mut host, &toast.path, DismissReason::Timeout)
        .expect("timeout runs");
    assert!(damage.is_some());
    assert_eq!(t.view.take_dismissed(), Some(DismissReason::Timeout));
    assert_eq!(t.field("toast"), Value::Bool(false));
    assert!(t.view.overlays().transients().next().is_none());
    // A second timeout for a layer that is gone is nothing.
    let none = t
        .view
        .dismiss_at(&BaseTokens, &t.device, &locale, &mut host, &toast.path, DismissReason::Timeout)
        .expect("runs");
    assert_eq!(none, None);
    assert_eq!(t.view.take_dismissed(), None);
}
