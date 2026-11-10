// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// The calculator's ⋮ menu (design artifacts: calculator C1–C3, C39): the button opens the kit's
// app menu, "clear all" resets the operation AND its expression line (the two-stage C pressed
// twice), "copy result" writes exactly what the display shows over the calculator's clipboard
// route — and both close the menu. The route itself is the manifest's CLIPBOARD grant
// (`tools/nxb-pack/tests/repo_bundles.rs` pins the calculator's caps).

mod common;

use common::scene_texts;
use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};

/// Records every service call; the clipboard write answers `true`.
#[derive(Default)]
struct Spy {
    calls: Vec<String>,
}

impl nexus_dsl_runtime::EffectHost for Spy {
    fn call(&mut self, svc: &str, method: &str, args: &[Value]) -> Result<Value, u32> {
        let arg = match args.first() {
            Some(Value::Str(s)) => s.clone(),
            _ => String::new(),
        };
        self.calls.push(format!("{svc}.{method}({arg})"));
        Ok(Value::Bool(true))
    }
}

struct Calc {
    view: View<'static>,
    device: FixtureEnv,
    symbols: Vec<String>,
    keys: Vec<u32>,
    host: Spy,
}

impl Calc {
    fn new() -> Self {
        let nxir: &'static [u8] = Box::leak(common::compile("calculator").into_boxed_slice());
        let symbols = common::program_symbols(nxir);
        let keys = common::program_i18n_keys(nxir);
        let device = FixtureEnv::desktop();
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        let view = View::mount(nxir, &nexus_theme_tokens::BaseTokens, &device, &locale)
            .expect("the calculator mounts");
        Self { view, device, symbols, keys, host: Spy::default() }
    }

    fn send(&mut self, event: &str, case: &str, payload: Vec<Value>) {
        common::dispatch_with_keys(
            &mut self.view,
            &self.device,
            &mut self.host,
            &self.symbols,
            &self.keys,
            event,
            case,
            payload,
        );
    }

    fn field(&self, store: &str, name: &str) -> Value {
        self.view.runtime.field(store, name).cloned().expect("a store field")
    }

    /// 12 + 3, an operation pending with its expression line.
    fn type_an_operation(&mut self) {
        self.send("CalcEvent", "Digit", vec![Value::Fx(1 << 32), Value::Str("1".into())]);
        self.send("CalcEvent", "Digit", vec![Value::Fx(2 << 32), Value::Str("2".into())]);
        self.send("CalcEvent", "Op", vec![Value::Str("+".into()), Value::Str("+".into())]);
        self.send("CalcEvent", "Digit", vec![Value::Fx(3 << 32), Value::Str("3".into())]);
    }
}

#[test]
fn the_menu_button_opens_the_two_rows() {
    let mut calc = Calc::new();
    calc.send("WindowEvent", "WinMenu", vec![Value::Str("more".into())]);
    let texts = scene_texts(&calc.view);
    assert!(texts.iter().any(|t| t == "Clear all"), "the clear-all row shows: {texts:?}");
    assert!(texts.iter().any(|t| t == "Copy result"), "the copy row shows: {texts:?}");
}

#[test]
fn clear_all_resets_the_operation_and_closes_the_menu() {
    let mut calc = Calc::new();
    calc.type_an_operation();
    assert_eq!(calc.field("CalcStore", "op"), Value::Str("+".into()));
    calc.send("WindowEvent", "WinMenu", vec![Value::Str("more".into())]);
    calc.send("CalcMenuEvent", "ClearAll", Vec::new());
    assert_eq!(calc.field("CalcStore", "disp"), Value::Str("0".into()));
    assert_eq!(calc.field("CalcStore", "op"), Value::Str(String::new()), "the operation is gone");
    assert_eq!(calc.field("CalcStore", "history"), Value::Str(String::new()), "and its line");
    assert_eq!(calc.field("WindowStore", "menu"), Value::Str(String::new()), "the menu closed");
    assert!(calc.host.calls.is_empty(), "clearing calls no service: {:?}", calc.host.calls);
}

#[test]
fn copy_result_writes_what_the_display_shows_and_closes_the_menu() {
    let mut calc = Calc::new();
    calc.type_an_operation();
    calc.send("WindowEvent", "WinMenu", vec![Value::Str("more".into())]);
    calc.send("CalcMenuEvent", "CopyResult", Vec::new());
    assert_eq!(calc.host.calls, vec!["clipboard.write(3)".to_string()], "the display's text");
    assert_eq!(calc.field("CalcMenuStore", "copied"), Value::Int(1));
    assert_eq!(calc.field("WindowStore", "menu"), Value::Str(String::new()), "the menu closed");
}
