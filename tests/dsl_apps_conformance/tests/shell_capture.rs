// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0068 (RFC-0095): the screenshot UI against the REAL compiled shell at the desktop
//! layout, with a fake capture service. Print (`on CaptureOpen`) begins a capture and opens
//! the overlay as the shell's modal — the live shell content leaves (the frozen frame shows
//! it); a drag draws a selection from the press, a drag inside moves it, a drag that starts on
//! the panel draws nothing; the window mode outlines the windows and a click picks one; the
//! shutter sends exactly the mode's arguments and the toast names the saved file; ESC thaws;
//! Shift+Print saves the screen without the overlay. Also the SSOT for the live lane's
//! injector targets (`tools/qmp_inject_modal.py`, capture phase).

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use nexus_dsl_runtime::interact::HandlerAction;
use nexus_dsl_runtime::{DismissReason, FixtureEnv, IdentityLocale, Value, View};
use nexus_layout_types::FxPx;
use nexus_theme_tokens::BaseTokens;

/// Display-space points at 1280×800 the live lane's injector drives: the drag that draws the
/// selection (press, release) and the shutter's centre.
pub const INJECT_CAPTURE_DRAG: ((i32, i32), (i32, i32)) = ((120, 140), (520, 420));
pub const INJECT_CAPTURE_SHUTTER: (i32, i32) = (640, 728);

/// The frozen frame the fake service answers: 1280×800, a back window and a front window.
const BACK: (i64, i64, i64, i64, i64) = (9, 700, 60, 500, 500);
const FRONT: (i64, i64, i64, i64, i64) = (7, 100, 80, 600, 400);

/// A fake capture service: records every call, answers `begin` with the frame above.
struct CapHost {
    syms: [u32; 7],
    calls: Vec<String>,
    begin_fails: bool,
}

impl CapHost {
    fn new(symbols: &[String]) -> Self {
        let sym = |n: &str| symbols.iter().position(|s| s == n).expect(n) as u32;
        let syms =
            [sym("w"), sym("h"), sym("front"), sym("windows"), sym("id"), sym("x"), sym("y")];
        Self { syms, calls: Vec::new(), begin_fails: false }
    }

    fn record(mut fields: Vec<(u32, Value)>) -> Value {
        fields.sort_by_key(|(s, _)| *s);
        Value::Record(fields)
    }

    fn window(&self, (id, x, y, w, h): (i64, i64, i64, i64, i64)) -> Value {
        let [sw, sh, _, _, sid, sx, sy] = self.syms;
        Self::record(vec![
            (sid, Value::Int(id)),
            (sx, Value::Int(x)),
            (sy, Value::Int(y)),
            (sw, Value::Int(w)),
            (sh, Value::Int(h)),
        ])
    }

    fn frame(&self) -> Value {
        let [sw, sh, sfront, swindows, ..] = self.syms;
        Self::record(vec![
            (sw, Value::Int(1280)),
            (sh, Value::Int(800)),
            (sfront, self.window(FRONT)),
            // Back to front, as the binding delivers them.
            (swindows, Value::List(vec![self.window(BACK), self.window(FRONT)])),
        ])
    }
}

impl nexus_dsl_runtime::EffectHost for CapHost {
    fn call(&mut self, service: &str, method: &str, args: &[Value]) -> Result<Value, u32> {
        let args: Vec<String> = args
            .iter()
            .map(|a| match a {
                Value::Str(s) => format!("{s:?}"),
                Value::Int(i) => i.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => String::from("?"),
            })
            .collect();
        self.calls.push(format!("{service}.{method}({})", args.join(", ")));
        match (service, method) {
            ("screencap", "begin") if self.begin_fails => Err(4),
            ("screencap", "begin") => Ok(self.frame()),
            ("screencap", "shoot" | "shot") => {
                Ok(Value::Str(String::from("Screenshot from 2026-10-09 14-32-05.png")))
            }
            ("screencap", "cancel") => Ok(Value::Bool(true)),
            ("bundlemgr", "enumerate") | ("clipboard", "list") => Ok(Value::List(Vec::new())),
            _ => Ok(Value::Bool(true)),
        }
    }
}

struct Rig {
    view: View<'static>,
    symbols: Vec<String>,
    keys: Vec<u32>,
    host: CapHost,
}

impl Rig {
    fn new() -> Self {
        let nxir: &'static [u8] = Box::leak(common::compile("desktop-shell").into_boxed_slice());
        let symbols = common::program_symbols(nxir);
        let keys = common::program_i18n_keys(nxir);
        let host = CapHost::new(&symbols);
        let view = {
            let locale = IdentityLocale { symbols: &symbols, keys: &keys };
            View::mount(nxir, &BaseTokens, &FixtureEnv::desktop(), &locale).expect("mounts")
        };
        Self { view, symbols, keys, host }
    }

    fn field(&self, name: &str) -> Value {
        self.view.runtime.field("SystemStore", name).cloned().expect(name)
    }

    fn int(&self, name: &str) -> i64 {
        match self.field(name) {
            Value::Int(i) => i,
            other => panic!("{name} is not an Int: {other:?}"),
        }
    }

    fn trigger(&mut self, name: &str) {
        let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
        let device = FixtureEnv::desktop();
        self.view
            .fire_trigger(&BaseTokens, &device, &locale, &mut self.host, name)
            .expect("trigger runs");
    }

    fn tap(&mut self, (x, y): (i32, i32)) {
        let boxes = common::layout_boxes(&self.view);
        let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
        let device = FixtureEnv::desktop();
        self.view
            .pointer_scrolled(
                &BaseTokens,
                &device,
                &locale,
                &mut self.host,
                &boxes,
                "Tap",
                FxPx::new(x),
                FxPx::new(y),
                None,
            )
            .expect("tap runs");
    }

    /// A drag from `from` to `to`, the way app-host drives it: `DragStart` hit-tested at the
    /// press, `DragMove` and `DragEnd` on the box that took it.
    fn drag(&mut self, from: (i32, i32), to: (i32, i32)) {
        let boxes = common::layout_boxes(&self.view);
        let Some(target) = self.view.hover_box_id_scrolled(
            &boxes,
            "DragStart",
            FxPx::new(from.0),
            FxPx::new(from.1),
            None,
        ) else {
            return;
        };
        let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
        let at = |p: (i32, i32)| FixtureEnv {
            drag: (p.0, p.1, from.0, from.1),
            ..FixtureEnv::desktop()
        };
        for (point, trigger) in [(from, "DragStart"), (to, "DragMove"), (to, "DragEnd")] {
            self.view
                .fire_on_box(&BaseTokens, &at(point), &locale, &mut self.host, target, trigger)
                .expect("drag runs");
        }
    }

    fn handler_center(&self, case: &str, payload: Option<&Value>) -> Option<(i32, i32)> {
        let boxes = common::layout_boxes(&self.view);
        let (e, c) = self.view.runtime.event_case("CaptureEvent", case).expect("case");
        let tap = self.symbols.iter().position(|s| s == "Tap").expect("Tap") as u32;
        let (box_id, _) = self.view.handlers().iter().find(|(_, h)| {
            h.trigger == tap
                && matches!(&h.action, HandlerAction::Dispatch { event, case, payload: p }
                    if *event == e && *case == c
                        && payload.is_none_or(|want| p.first() == Some(want)))
        })?;
        let b = boxes.iter().find(|b| b.node_id == *box_id).expect("box");
        Some((b.rect.x.0 + b.rect.width.0 / 2, b.rect.y.0 + b.rect.height.0 / 2))
    }

    fn hole(&self) -> (i64, i64, i64, i64) {
        (self.int("capHoleX"), self.int("capHoleY"), self.int("capHoleW"), self.int("capHoleH"))
    }
}

#[test]
fn print_opens_the_overlay_over_the_frozen_frame_and_a_drag_draws_the_selection() {
    let mut rig = Rig::new();
    assert!(common::scene_texts(&rig.view).iter().any(|t| t == "--:--"), "the clock shows");
    rig.trigger("CaptureOpen");
    assert_eq!(rig.host.calls, vec!["screencap.begin()"]);
    assert_eq!(rig.field("capOpen"), Value::Bool(true));
    assert_eq!(rig.view.overlays().modal_depth(), 1, "the capture layer is the shell's modal");
    assert!(
        !common::scene_texts(&rig.view).iter().any(|t| t == "--:--"),
        "the live shell leaves: the frozen frame already shows it"
    );
    assert_eq!(rig.hole(), (320, 200, 640, 400), "a centred selection of half the screen");

    let (press, release) = INJECT_CAPTURE_DRAG;
    rig.drag(press, release);
    assert_eq!(rig.hole(), (120, 140, 400, 280), "drawn from the press to the release");
    // A drag inside the selection moves it; the size stays.
    rig.drag((300, 300), (330, 320));
    assert_eq!(rig.hole(), (150, 160, 400, 280));
    // A drag from the bottom-right handle resizes from the top-left corner.
    rig.drag((548, 438), (600, 500));
    assert_eq!(rig.hole(), (150, 160, 450, 340));

    let shutter = rig.handler_center("CapShoot", None).expect("shutter");
    assert_eq!(shutter, INJECT_CAPTURE_SHUTTER, "the injector's shutter target moved");
    rig.tap(shutter);
    assert_eq!(
        rig.host.calls.last().unwrap(),
        "screencap.shoot(\"area\", 150, 160, 450, 340, false, \"Screenshot from\")"
    );
    assert_eq!(rig.field("capOpen"), Value::Bool(false));
    assert_eq!(rig.field("toast"), Value::Str(String::from("capture")));
    let texts = common::scene_texts(&rig.view);
    assert!(texts.iter().any(|t| t == "Screenshot from 2026-10-09 14-32-05.png"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "--:--"), "the live shell is back");
}

#[test]
fn the_window_mode_picks_a_window_in_place_and_shoots_it_with_the_pointer() {
    let mut rig = Rig::new();
    rig.trigger("CaptureOpen");
    let window = Value::Str(String::from("window"));
    let mode = rig.handler_center("CapMode", Some(&window)).expect("window mode button");
    rig.tap(mode);
    assert_eq!(rig.hole(), (100, 80, 600, 400), "the front window is picked to start with");
    // The back window's outline peeks out right of the front one.
    let back = rig.handler_center("CapPickWindow", Some(&Value::Int(BACK.0))).expect("back");
    assert!(back.0 > 700, "the back window's outline sits in place: {back:?}");
    rig.tap((1000, 300));
    assert_eq!(rig.hole(), (700, 60, 500, 500), "a click picks the window under it");
    let pointer = rig.handler_center("CapTogglePointer", None).expect("pointer toggle");
    rig.tap(pointer);
    let shutter = rig.handler_center("CapShoot", None).expect("shutter");
    rig.tap(shutter);
    assert_eq!(
        rig.host.calls.last().unwrap(),
        "screencap.shoot(\"window\", 9, 0, 0, 0, true, \"Screenshot from\")"
    );
}

#[test]
fn test_reject_a_drag_on_the_panel_draws_nothing_and_esc_thaws() {
    let mut rig = Rig::new();
    rig.trigger("CaptureOpen");
    let before = rig.hole();
    let shutter = rig.handler_center("CapShoot", None).expect("shutter");
    rig.drag(shutter, (200, 200));
    assert_eq!(rig.hole(), before, "a drag that starts on the panel draws no selection");
    let locale = IdentityLocale { symbols: &rig.symbols, keys: &rig.keys };
    rig.view
        .dismiss_top(
            &BaseTokens,
            &FixtureEnv::desktop(),
            &locale,
            &mut rig.host,
            DismissReason::Escape,
        )
        .expect("esc");
    assert_eq!(rig.host.calls.last().unwrap(), "screencap.cancel()");
    assert_eq!(rig.field("capOpen"), Value::Bool(false));
    assert_eq!(rig.view.overlays().modal_depth(), 0);
}

/// The film switch is drawn greyed out and takes no input until recording exists (TASK-0105):
/// `.disabled(true)` drops its handler, so a press beside the photo button lands on the panel's
/// absorber — no mode, no service call.
#[test]
fn test_reject_the_film_button_takes_no_input() {
    let mut rig = Rig::new();
    rig.trigger("CaptureOpen");
    let boxes = common::layout_boxes(&rig.view);
    let (shutter_x, _) = rig.handler_center("CapShoot", None).expect("shutter");
    let noop = rig.view.runtime.event_case("CaptureEvent", "CapNoop").expect("CapNoop");
    let tap = rig.symbols.iter().position(|s| s == "Tap").expect("Tap") as u32;
    let photo = rig
        .view
        .handlers()
        .iter()
        .filter(|(_, h)| {
            h.trigger == tap
                && matches!(&h.action, HandlerAction::Dispatch { event, case, .. }
                    if (*event, *case) == noop)
        })
        .filter_map(|(id, _)| boxes.iter().find(|b| b.node_id == *id))
        .find(|b| b.rect.width.0 == b.rect.height.0 && b.rect.x.0 < shutter_x)
        .expect("the photo button (a round CapNoop handler left of the shutter)")
        .rect;
    let film = boxes
        .iter()
        .find(|b| {
            b.rect.y == photo.y
                && b.rect.width == photo.width
                && b.rect.height == photo.height
                && b.rect.x.0 > photo.x.0 + photo.width.0 / 2
                && b.rect.x.0 < shutter_x
        })
        .expect("the film button beside the photo button")
        .rect;
    let center = (film.x.0 + film.width.0 / 2, film.y.0 + film.height.0 / 2);
    let hit = rig
        .view
        .hover_box_id_scrolled(&boxes, "Tap", FxPx::new(center.0), FxPx::new(center.1), None)
        .and_then(|id| boxes.iter().find(|b| b.node_id == id))
        .expect("the panel absorbs");
    assert!(hit.rect.width.0 > film.width.0 * 4, "the press lands on the panel, not the button");
    let (calls, mode) = (rig.host.calls.len(), rig.field("capMode"));
    rig.tap(center);
    assert_eq!(rig.host.calls.len(), calls, "no service call");
    assert_eq!(rig.field("capMode"), mode, "no mode change");
}

#[test]
fn test_reject_a_refused_begin_opens_nothing() {
    let mut rig = Rig::new();
    rig.host.begin_fails = true;
    rig.trigger("CaptureOpen");
    assert_eq!(rig.field("capOpen"), Value::Bool(false));
    assert_eq!(rig.view.overlays().modal_depth(), 0, "no overlay over a live screen");
}

#[test]
fn shift_print_saves_the_screen_without_the_overlay() {
    let mut rig = Rig::new();
    rig.trigger("CaptureScreen");
    assert_eq!(rig.host.calls, vec!["screencap.shot(\"screen\", false, \"Screenshot from\")"]);
    assert_eq!(rig.field("capOpen"), Value::Bool(false));
    assert_eq!(rig.field("toast"), Value::Str(String::from("capture")));
    rig.trigger("CaptureWindow");
    assert_eq!(
        rig.host.calls.last().unwrap(),
        "screencap.shot(\"window\", false, \"Screenshot from\")"
    );
}
