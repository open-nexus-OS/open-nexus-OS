// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! `{0:n}` / `{0:ns}` locale number formatting (TASK-0314).
//!
//! `Value::Fx` used to reach text as `raw >> 32` — the integer part — so every
//! fraction was silently dropped. These pin the replacement, including the two
//! properties a calculator actually depends on: a typed `1,50` keeps its
//! trailing zero, and a plain `{0}` still renders exactly as before.

use nexus_dsl_runtime::Value;

/// Formats `template` with `args` — the same function every catalog lookup
/// funnels through.
fn fmt(template: &str, args: &[Value]) -> String {
    nexus_dsl_runtime::i18n::format_template(template, args)
}

const FX_ONE: i64 = 1 << 32;

fn fx(int: i64, num: i64, den: i64) -> i64 {
    int * FX_ONE + (num * FX_ONE) / den
}

#[test]
fn german_separators_group_and_decimalise() {
    assert_eq!(fmt("{0:n,.}", &[Value::Int(1_000_000)]), "1.000.000");
    assert_eq!(fmt("{0:n,.}", &[Value::Fx(fx(1234, 1, 2))]), "1.234,5");
    assert_eq!(fmt("{0:n,.}", &[Value::Fx(fx(9876, 0, 1))]), "9.876");
}

#[test]
fn english_separators_are_the_mirror_image() {
    assert_eq!(fmt("{0:n.,}", &[Value::Int(1_000_000)]), "1,000,000");
    assert_eq!(fmt("{0:n.,}", &[Value::Fx(fx(1234, 1, 2))]), "1,234.5");
}

#[test]
fn fractions_survive_and_are_bounded() {
    // 1/4 is exact in binary; 1/3 is not and must stop, not spin.
    assert_eq!(fmt("{0:n.}", &[Value::Fx(fx(0, 1, 4))]), "0.25");
    let third = fmt("{0:n.}", &[Value::Fx(fx(0, 1, 3))]);
    assert!(third.starts_with("0.333"), "got {third}");
    assert!(third.len() <= 2 + 10, "fraction must be bounded, got {third}");
}

#[test]
fn negatives_keep_their_sign_and_fraction() {
    assert_eq!(fmt("{0:n,.}", &[Value::Fx(-fx(1234, 1, 2))]), "-1.234,5");
    assert_eq!(fmt("{0:n,.}", &[Value::Int(-1000)]), "-1.000");
    // Negative zero is still zero, without a stray sign.
    assert_eq!(fmt("{0:n,.}", &[Value::Fx(0)]), "0");
}

/// The property an `Fx` cannot express: `1.50` and `1.5` are the same NUMBER,
/// but a user typing the second zero must see it appear.
#[test]
fn a_typed_string_keeps_its_trailing_zeros() {
    assert_eq!(fmt("{0:ns,.}", &[Value::Str("1234.50".into())]), "1.234,50");
    assert_eq!(fmt("{0:ns,.}", &[Value::Str("0.100".into())]), "0,100");
    // A lone trailing separator survives, so `12,` reads while typing.
    assert_eq!(fmt("{0:ns,.}", &[Value::Str("12.".into())]), "12,");
    assert_eq!(fmt("{0:ns,.}", &[Value::Str("-1234.5".into())]), "-1.234,5");
}

/// A non-numeric payload in a numeric slot passes through instead of being
/// mangled — the error string has to survive the same template.
#[test]
fn non_numeric_text_passes_through() {
    assert_eq!(fmt("{0:ns,.}", &[Value::Str("Fehler".into())]), "Fehler");
}

/// Everything that worked before must still work: no spec = old behaviour.
#[test]
fn plain_interpolation_is_unchanged() {
    assert_eq!(fmt("{0}", &[Value::Int(42)]), "42");
    assert_eq!(fmt("{0}", &[Value::Str("hi".into())]), "hi");
    assert_eq!(fmt("{0}", &[Value::Bool(true)]), "true");
    // An Fx without a spec keeps the historical integer-part rendering.
    assert_eq!(fmt("{0}", &[Value::Fx(fx(7, 1, 2))]), "7");
    // A malformed spec degrades to plain interpolation, never to a panic.
    assert_eq!(fmt("{0:zz}", &[Value::Int(5)]), "5");
    assert_eq!(fmt("no args here", &[]), "no args here");
}
