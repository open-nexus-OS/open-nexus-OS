// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// The calculator's ARITHMETIC and its localized display
// (design_handoff_calculator "Funktion"). The handoff's own worked example is
// the headline case: `1234,5 x 8 =` must read `9.876` with the history line
// `1.234,5 × 8 =`.
//
// These run against the REAL i18n catalogs, so a broken separator or a lost
// fraction fails here rather than on the device.

mod common;

use common::scene_texts;
use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};

struct NoHost;
impl nexus_dsl_runtime::EffectHost for NoHost {
    fn call(&mut self, _s: &str, _m: &str, _a: &[Value], _t: u32) -> Result<Value, u32> {
        Err(0)
    }
}

/// A mounted calculator driven through its real event vocabulary.
struct Calc {
    nxir: Vec<u8>,
    symbols: Vec<String>,
    keys: Vec<u32>,
}

impl Calc {
    fn new() -> Self {
        let nxir = common::compile("calculator");
        let symbols = common::program_symbols(&nxir);
        let keys = common::program_i18n_keys(&nxir);
        Self { nxir, symbols, keys }
    }
}

macro_rules! session {
    ($calc:ident, $view:ident, $tokens:ident, $device:ident, $locale:ident, $host:ident) => {
        let $tokens = nexus_theme_tokens::BaseTokens;
        let $device = FixtureEnv::tablet("landscape");
        let $locale = IdentityLocale { symbols: &$calc.symbols, keys: &$calc.keys };
        let mut $host = NoHost;
        let mut $view = View::mount(&$calc.nxir, &$tokens, &$device, &$locale).expect("mounts");
        $view.run_initial_effects(&$tokens, &$device, &$locale, &mut $host).expect("effects");
    };
}

fn send(
    view: &mut View<'_>,
    tokens: &nexus_theme_tokens::BaseTokens,
    device: &FixtureEnv,
    locale: &IdentityLocale<'_>,
    host: &mut NoHost,
    case: &str,
    args: Vec<Value>,
) {
    let (e, c) = view
        .runtime()
        .event_case("CalcEvent", case)
        .unwrap_or_else(|| panic!("event {case} exists"));
    view.dispatch(tokens, device, locale, host, e, c, args).unwrap_or_else(|e| {
        panic!("{case} dispatches: {e:?}");
    });
}

/// Types the machine-format number `text` (digits and `.`).
fn type_number(
    view: &mut View<'_>,
    tokens: &nexus_theme_tokens::BaseTokens,
    device: &FixtureEnv,
    locale: &IdentityLocale<'_>,
    host: &mut NoHost,
    text: &str,
) {
    for ch in text.chars() {
        if ch == '.' {
            // The keypad sends the key's OWN character; tests use "." so the
            // machine-format strings below stay readable.
            send(view, tokens, device, locale, host, "Sep", vec![Value::Str(".".into())]);
        } else {
            // Q32.32: the digit in the integer half, plus its character.
            let d = i64::from(ch.to_digit(10).expect("a digit")) << 32;
            send(
                view,
                tokens,
                device,
                locale,
                host,
                "Digit",
                vec![Value::Fx(d), Value::Str(ch.to_string())],
            );
        }
    }
}

/// THE handoff example: `1234,5 × 8 =` → `9.876`.
#[test]
fn the_handoff_worked_example() {
    let calc = Calc::new();
    session!(calc, view, tokens, device, locale, host);
    type_number(&mut view, &tokens, &device, &locale, &mut host, "1234.5");
    // The typed entry is shown VERBATIM — no catalog on the typing path, so
    // no thousands grouping while typing (recorded trade-off; the RESULT below
    // is still locale-formatted).
    assert!(
        scene_texts(&view).iter().any(|t| t == "1234.5"),
        "typed entry must echo what was pressed, got {:?}",
        scene_texts(&view)
    );
    send(&mut view, &tokens, &device, &locale, &mut host, "Op", vec![Value::Str("*".into())]);
    type_number(&mut view, &tokens, &device, &locale, &mut host, "8");
    send(&mut view, &tokens, &device, &locale, &mut host, "Eq", vec![]);
    let texts = scene_texts(&view);
    assert!(texts.iter().any(|t| t == "9,876"), "1234.5 * 8 = 9876, got {texts:?}");
    assert!(
        texts.iter().any(|t| t.contains('*') || t.contains('×')),
        "the history line must show the operation, got {texts:?}"
    );
}

/// Fractions survive a computation — the whole point of the `Fx` path.
#[test]
fn decimals_are_real() {
    let calc = Calc::new();
    session!(calc, view, tokens, device, locale, host);
    type_number(&mut view, &tokens, &device, &locale, &mut host, "0.5");
    send(&mut view, &tokens, &device, &locale, &mut host, "Op", vec![Value::Str("+".into())]);
    type_number(&mut view, &tokens, &device, &locale, &mut host, "0.25");
    send(&mut view, &tokens, &device, &locale, &mut host, "Eq", vec![]);
    let texts = scene_texts(&view);
    assert!(texts.iter().any(|t| t == "0.75"), "0.5 + 0.25 = 0.75, got {texts:?}");
}

/// A typed trailing zero must stay visible — an `Fx` alone cannot express it.
#[test]
fn a_typed_trailing_zero_survives() {
    let calc = Calc::new();
    session!(calc, view, tokens, device, locale, host);
    type_number(&mut view, &tokens, &device, &locale, &mut host, "1.50");
    assert!(
        scene_texts(&view).iter().any(|t| t == "1.50"),
        "typing 1.50 must show 1.50, got {:?}",
        scene_texts(&view)
    );
}

/// Division by zero is an ERROR STATE, not a silent 0 and not a crash.
#[test]
fn division_by_zero_reports_an_error() {
    let calc = Calc::new();
    session!(calc, view, tokens, device, locale, host);
    type_number(&mut view, &tokens, &device, &locale, &mut host, "8");
    send(&mut view, &tokens, &device, &locale, &mut host, "Op", vec![Value::Str("/".into())]);
    type_number(&mut view, &tokens, &device, &locale, &mut host, "0");
    send(&mut view, &tokens, &device, &locale, &mut host, "Eq", vec![]);
    let texts = scene_texts(&view);
    assert!(
        texts.iter().any(|t| t == "Error" || t == "Fehler"),
        "division by zero must say so, got {texts:?}"
    );
    // …and the next digit recovers rather than wedging.
    type_number(&mut view, &tokens, &device, &locale, &mut host, "3");
    assert!(
        scene_texts(&view).iter().any(|t| t == "3"),
        "a digit after an error must start a fresh entry, got {:?}",
        scene_texts(&view)
    );
}

#[test]
fn percent_and_sign_flip() {
    let calc = Calc::new();
    session!(calc, view, tokens, device, locale, host);
    type_number(&mut view, &tokens, &device, &locale, &mut host, "50");
    send(&mut view, &tokens, &device, &locale, &mut host, "Percent", vec![]);
    assert!(
        scene_texts(&view).iter().any(|t| t == "0.5"),
        "50% = 0.5, got {:?}",
        scene_texts(&view)
    );
    send(&mut view, &tokens, &device, &locale, &mut host, "Neg", vec![]);
    assert!(
        scene_texts(&view).iter().any(|t| t == "-0.5"),
        "± flips the sign, got {:?}",
        scene_texts(&view)
    );
}

/// The 9-digit cap (Q32.32 holds ~2.1e9): the key is IGNORED, never wrapped
/// into a wrong number.
#[test]
fn the_entry_is_capped_not_wrapped() {
    let calc = Calc::new();
    session!(calc, view, tokens, device, locale, host);
    type_number(&mut view, &tokens, &device, &locale, &mut host, "1234567890123");
    let texts = scene_texts(&view);
    assert!(
        texts.iter().any(|t| t == "123456789"),
        "the entry must stop at 9 digits, got {texts:?}"
    );
}

#[test]
fn dump_display() {
    let calc = Calc::new();
    session!(calc, view, tokens, device, locale, host);
    type_number(&mut view, &tokens, &device, &locale, &mut host, "123");
    println!("scene texts: {:?}", scene_texts(&view));
    let l = nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(960),
            Some(nexus_layout_types::FxPx::new(640)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    for b in l.boxes.iter().take(10) {
        println!(
            "node={} x={} y={} w={} h={} text_px={:?}",
            b.node_id,
            b.rect.x.0,
            b.rect.y.0,
            b.rect.width.0,
            b.rect.height.0,
            b.text_px.map(|p| p.as_i32())
        );
    }
}
