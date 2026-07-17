// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Accept/reject corpus with stable diagnostic codes (the `nx dsl explain`
//! contract): each reject case pins its code; message text is free to evolve.

use nexus_dsl_core::{check_file, parse_file, DiagCode};

fn parse_err(src: &str) -> DiagCode {
    parse_file(src).expect_err("must be rejected").code
}

fn check_codes(src: &str) -> Vec<DiagCode> {
    let file = parse_file(src).expect("parses");
    let (_, diags) = check_file(&file);
    diags.iter().map(|d| d.code).collect()
}

#[test]
fn parser_rejects_carry_stable_codes() {
    assert_eq!(parse_err("Store S { x: Int = , }"), DiagCode::UnexpectedToken);
    assert_eq!(parse_err("Routes { \"x\" -> P; }"), DiagCode::InvalidRoutePath);
    assert_eq!(parse_err("reduce E { }"), DiagCode::EmptyMatch);
    assert_eq!(
        parse_err("Page P { Text { value: \"a\", value: \"b\" } }"),
        DiagCode::DuplicateProp
    );
    assert_eq!(parse_err("Store S { x: Int, } trailing"), DiagCode::UnexpectedToken);
    assert_eq!(parse_err("$foo"), DiagCode::UnexpectedChar);
    assert_eq!(parse_err("Store S { s: Str = \"unterminated, }"), DiagCode::UnterminatedString);
}

#[test]
fn checker_rejects_carry_stable_codes() {
    // Purity.
    assert!(check_codes(
        "Store S { a: Int = 0, } Event E { X, } reduce E { X => { dispatch(X); }, }"
    )
    .contains(&DiagCode::ReducerImpure));
    // Exhaustiveness.
    assert!(check_codes(
        "Store S { a: Int = 0, } Event E { X, Y, } reduce E { X => state.a = 1, }"
    )
    .contains(&DiagCode::NotExhaustive));
    // Keys + labels.
    assert!(check_codes(
        "Page P { List($state.xs) { x in Text(x.n) } } Store S { xs: List<T> = [], }"
    )
    .contains(&DiagCode::MissingKey));
    assert!(check_codes("Page P { Toggle { checked: true } }").contains(&DiagCode::MissingLabel));
    // Unknown widget/modifier.
    assert!(check_codes("Page P { Wobble { } }").contains(&DiagCode::UnknownWidget));
    assert!(check_codes("Page P { Text(\"x\").sparkle(2) }").contains(&DiagCode::UnknownModifier));
    // Wrong modifier arity.
    assert!(check_codes("Page P { Text(\"x\").padding(1, 2) }").contains(&DiagCode::WrongArity));
    // Duplicate routes.
    assert!(check_codes("Page H { Stack { } } Routes { \"/\" -> H; \"/\" -> H; }")
        .contains(&DiagCode::DuplicateRoute));
    // Unknown device field.
    assert!(check_codes(
        "Page P { Stack { if device.flavor == sweet { Text(\"a\") } else { Text(\"b\") } } }"
    )
    .contains(&DiagCode::UnknownField));
}

#[test]
fn accept_corpus_stays_green() {
    for src in [
        // Minimal page.
        "Page P { Stack { } }",
        // Bare widgets + inline modifiers on blockless nodes.
        "Page P { Stack { Spacer Text(\"hi\").textSize(sm).fg(accent) } }",
        // Component with props, emit handler after modifiers.
        r#"
Component Row {
    props: { title: Str, onOpen: EventRef, }
    Stack { Text($props.title) }
    .gap(2)
    on Tap -> emit($props.onOpen)
}
Page P { Stack { Row { title: "x", onOpen: Open } } }
Event E { Open, }
Store S { a: Int = 0, }
reduce E { Open => state.a += 1, }
"#,
        // for over a literal list (bounded) + match view.
        r#"
Store S { mode: Str = "a", }
Page P {
    Stack {
        for n in [1, 2, 3] {
            Text("dot")
        }
    }
}
"#,
    ] {
        let file = parse_file(src).unwrap_or_else(|e| panic!("corpus parse: {e:?}\n{src}"));
        let (_, diags) = check_file(&file);
        assert!(!nexus_dsl_core::has_errors(&diags), "corpus must check clean: {diags:?}\n{src}");
    }
}
