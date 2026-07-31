// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Locale-aware number rendering for i18n templates — the `{0:n}` /
//! `{0:ns}` format specs (TASK-0314).
//!
//! WHY THIS EXISTS: `Value::Fx` reached text as `raw >> 32`, i.e. the integer
//! part, so every fraction was silently dropped on the way to the screen. A
//! calculator cannot be written against that, and neither can a price, a
//! duration or a file size.
//!
//! WHY IT IS NOT A NEW DSL EXPRESSION: a `@n(x)` builtin would have to touch
//! the AST, the parser, the checker, the lowering pass AND `ui_ir.capnp` — a
//! wire-format change for what is really a formatting concern. A template
//! already goes through the locale catalog, and the catalog is per-locale, so
//! the SEPARATORS ride in the spec and each catalog states its own:
//!
//! ```text
//! de.json  "calc.value": "{0:n,.}"    -> 1.234,5
//! en.json  "calc.value": "{0:n.,}"    -> 1,234.5
//! ```
//!
//! Two kinds, because a calculator needs both:
//!   `n`  format a NUMBER  (Int/Fx) — the result of a computation;
//!   `ns` format a numeric STRING  — what the user has typed so far, where
//!        `1.50` must keep its trailing zero and a lone trailing `.` must
//!        survive. Re-deriving that from an `Fx` is impossible: 1.50 and 1.5
//!        are the same number.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/number_format.rs

use alloc::string::String;

/// Fraction digits kept for an `Fx`. Q32.32 resolves 2^-32 ≈ 2.3e-10, so the
/// tenth digit is already noise: rendering it turns the nearest representable
/// 0.1 into `0.0999999999`. Nine digits with ROUNDING gives `0.1`.
const MAX_FRACTION_DIGITS: u32 = 9;

/// A parsed `n` / `ns` format spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NumSpec {
    /// `true` for `ns` (format an already-typed numeric string).
    pub(crate) from_string: bool,
    pub(crate) decimal: char,
    pub(crate) group: Option<char>,
}

impl NumSpec {
    /// Parses the part AFTER the `:` of `{0:…}`. `None` = not a number spec,
    /// so the caller falls back to plain interpolation.
    ///
    /// `n` / `ns` alone keep the machine separators (`.`, no grouping); two
    /// trailing characters give decimal and group explicitly.
    pub(crate) fn parse(spec: &str) -> Option<Self> {
        let (from_string, rest) = if let Some(rest) = spec.strip_prefix("ns") {
            (true, rest)
        } else if let Some(rest) = spec.strip_prefix('n') {
            (false, rest)
        } else {
            return None;
        };
        let mut chars = rest.chars();
        let decimal = chars.next().unwrap_or('.');
        let group = chars.next();
        // A third character means the author meant something else entirely.
        if chars.next().is_some() {
            return None;
        }
        Some(NumSpec { from_string, decimal, group })
    }
}

/// Inserts `group` every three digits from the right.
fn group_digits(digits: &str, group: Option<char>, out: &mut String) {
    match group {
        None => out.push_str(digits),
        Some(sep) => {
            let n = digits.len();
            for (i, ch) in digits.chars().enumerate() {
                if i > 0 && (n - i) % 3 == 0 {
                    out.push(sep);
                }
                out.push(ch);
            }
        }
    }
}

/// Renders an integer with grouping.
pub(crate) fn format_int(value: i64, spec: NumSpec) -> String {
    let mut out = String::new();
    if value < 0 {
        out.push('-');
    }
    // `unsigned_abs` so i64::MIN does not overflow the negation.
    let digits = itoa(value.unsigned_abs());
    group_digits(&digits, spec.group, &mut out);
    out
}

/// Renders a Q32.32 fixed-point value with grouping and up to
/// [`MAX_FRACTION_DIGITS`] fraction digits, trailing zeros trimmed.
pub(crate) fn format_fx(raw: i64, spec: NumSpec) -> String {
    let negative = raw < 0;
    // The magnitude, so the fraction extraction below never sees a negative
    // remainder (two's complement makes -1.5 into int -2 + fraction .5).
    let magnitude = raw.unsigned_abs();
    let mut int_part = magnitude >> 32;
    let fraction = magnitude & 0xFFFF_FFFF;

    // ROUND, do not truncate: the nearest Q32.32 value to 0.1 is a hair BELOW
    // it, so truncating printed `0.0999999999`. Scale to the digit budget with
    // a half-ULP nudge, in u128 so the shift cannot overflow.
    let pow10 = 10u64.pow(MAX_FRACTION_DIGITS);
    let scaled = ((u128::from(fraction) * u128::from(pow10) + (1u128 << 31)) >> 32) as u64;
    // The nudge can carry all the way into the integer part (0.9999999999).
    let (mut frac_units, carry) = if scaled >= pow10 { (0, 1) } else { (scaled, 0) };
    int_part += carry;

    let mut digits = String::new();
    if frac_units != 0 {
        let mut place = pow10 / 10;
        while place > 0 {
            digits.push((b'0' + (frac_units / place) as u8) as char);
            frac_units %= place;
            place /= 10;
        }
        while digits.ends_with('0') {
            digits.pop();
        }
    }

    let mut out = String::new();
    if negative && (int_part != 0 || !digits.is_empty()) {
        out.push('-');
    }
    group_digits(&itoa(int_part), spec.group, &mut out);
    if !digits.is_empty() {
        out.push(spec.decimal);
        out.push_str(&digits);
    }
    out
}

/// Re-renders a MACHINE numeric string (`-1234.50`, `12.`) in the locale's
/// separators, preserving the fraction EXACTLY as typed.
///
/// This is the half an `Fx` cannot do: `1.50` and `1.5` are one number, but a
/// user typing the second zero must see it appear.
pub(crate) fn format_num_str(text: &str, spec: NumSpec) -> String {
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (int_digits, fraction) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    // Anything that is not a plain numeral is passed through untouched rather
    // than mangled — an error string ("Fehler") must survive the same slot.
    if int_digits.is_empty() && fraction.is_none() {
        return String::from(text);
    }
    if !int_digits.chars().all(|c| c.is_ascii_digit())
        || !fraction.unwrap_or("").chars().all(|c| c.is_ascii_digit())
    {
        return String::from(text);
    }
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    group_digits(if int_digits.is_empty() { "0" } else { int_digits }, spec.group, &mut out);
    if let Some(f) = fraction {
        out.push(spec.decimal);
        out.push_str(f);
    }
    out
}

/// `u64` → decimal, allocation-light and `no_std`-safe.
fn itoa(mut value: u64) -> String {
    if value == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while value > 0 {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    String::from(core::str::from_utf8(&buf[i..]).unwrap_or("0"))
}
