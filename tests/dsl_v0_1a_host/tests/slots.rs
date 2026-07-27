// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0084 slots — the frontend contract: what parses, what the checker
//! rejects (with its stable code), and that the formatter round-trips the new
//! syntax. Runtime behavior (caller scope, splice-not-wrap) is proven in
//! `userspace/dsl/runtime/tests/slots.rs` once Phase 3 lands.

use nexus_dsl_core::{check_file, format_file, parse_file, DiagCode};

fn check_codes(src: &str) -> Vec<DiagCode> {
    let file = parse_file(src).unwrap_or_else(|e| panic!("parses: {e:?}\n{src}"));
    let (_, diags) = check_file(&file);
    diags.iter().map(|d| d.code).collect()
}

fn assert_clean(src: &str) {
    let file = parse_file(src).unwrap_or_else(|e| panic!("parses: {e:?}\n{src}"));
    let (_, diags) = check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "must check clean: {diags:?}\n{src}");
}

/// A scaffold shaped like the one `window-kit` needs: per-region panel opt-in,
/// which puts the SAME placeholder in both arms of an `if`.
const SCAFFOLD: &str = r#"
Component Frame {
    props: {
        contentPanel: Bool,
    }
    slot sidebar
    slot content
    Stack {
        Stack {
            Slot sidebar
        }
        if $props.contentPanel {
            Panel {
                Slot content
            }
        } else {
            Stack {
                Slot content
            }
        }
    }
}
"#;

/// `SCAFFOLD` + a page body that uses it.
fn page(body: &str) -> String {
    format!("{SCAFFOLD}\nPage P {{\n{body}\n}}\n")
}

// ---------------------------------------------------------------- accepts

#[test]
fn scaffold_and_callsite_check_clean() {
    assert_clean(&page(
        r#"    Frame { contentPanel: true } {
        sidebar { Text("nav") }
        content { Panel { Text("a") } Panel { Text("b") } }
    }"#,
    ));
}

#[test]
fn slots_may_be_left_unbound() {
    // An unbound slot is silence, not an error — that is how a scaffold hides
    // a region (RFC-0084 §Failure model).
    assert_clean(&page(r#"    Frame { contentPanel: false } { content { Text("only") } }"#));
    assert_clean(&page("    Frame { contentPanel: false } {}"));
}

#[test]
fn slot_is_contextual_not_a_keyword() {
    // `slot` stays usable as a prop and a state field name — that is the whole
    // reason it is not a lexer keyword.
    assert_clean(
        r#"
Component NameProbe {
    props: {
        slot: Str,
    }
    state: {
        slot2: Int = 0,
    }
    Text($props.slot)
}

Page P {
    NameProbe { slot: "a" }
}
"#,
    );
}

#[test]
fn slot_followed_by_a_node_stays_two_siblings() {
    // `Slot` only reads as a placeholder before a BARE identifier; a node of
    // its own after it keeps the old two-widget reading (which then fails as
    // an unknown widget, exactly as it did before slots existed).
    let codes = check_codes(r#"Page P { Stack { Slot Text("x") } }"#);
    assert!(codes.contains(&DiagCode::UnknownWidget), "{codes:?}");
}

// ---------------------------------------------------------------- rejects

#[test]
fn reject_unknown_slot_at_callsite() {
    // Rule 1.
    assert!(page(r#"    Frame { contentPanel: true } { footer { Text("x") } }"#)
        .pipe(check_codes)
        .contains(&DiagCode::UnknownSlot));
}

#[test]
fn reject_slot_bound_twice() {
    // Rule 2.
    assert!(page(
        r#"    Frame { contentPanel: true } { content { Text("a") } content { Text("b") } }"#
    )
    .pipe(check_codes)
    .contains(&DiagCode::DuplicateDefinition));
}

#[test]
fn reject_slot_block_on_a_widget() {
    // Rule 3.
    assert!(page(r#"    Stack { } { content { Text("x") } }"#)
        .pipe(check_codes)
        .contains(&DiagCode::SlotShape));
}

#[test]
fn reject_slot_block_on_a_slotless_component() {
    // Rule 4.
    let src = r#"
Component Leaf {
    props: {
        label: Str,
    }
    Text($props.label)
}

Page P {
    Leaf { label: "x" } { content { Text("y") } }
}
"#;
    assert!(check_codes(src).contains(&DiagCode::SlotShape));
}

#[test]
fn reject_placeholder_for_an_undeclared_slot() {
    // Rule 5.
    let src = r#"
Component Frame {
    slot content
    Stack {
        Slot missing
    }
}

Page P {
    Frame { } { content { Text("x") } }
}
"#;
    assert!(check_codes(src).contains(&DiagCode::UnknownSlot));
}

#[test]
fn reject_placeholder_in_a_page() {
    // Rule 6 — a page has no caller to fill it.
    assert!(check_codes("Page P { Stack { Slot content } }").contains(&DiagCode::SlotShape));
}

#[test]
fn reject_slot_declared_twice() {
    // Rule 7.
    let src = r#"
Component Frame {
    slot content
    slot content
    Stack {
        Slot content
    }
}

Page P {
    Frame { } { content { Text("x") } }
}
"#;
    assert!(check_codes(src).contains(&DiagCode::DuplicateDefinition));
}

#[test]
fn reject_slot_colliding_with_a_prop_or_state_field() {
    // Rule 8 — `$props.content` and `Slot content` must not race for a name.
    let prop_clash = r#"
Component Frame {
    props: {
        content: Str,
    }
    slot content
    Stack {
        Slot content
    }
}

Page P {
    Frame { content: "x" } { content { Text("y") } }
}
"#;
    assert!(check_codes(prop_clash).contains(&DiagCode::DuplicateDefinition));

    let state_clash = r#"
Component Frame {
    state: {
        content: Int = 0,
    }
    slot content
    Stack {
        Slot content
    }
}

Page P {
    Frame { } { content { Text("y") } }
}
"#;
    assert!(check_codes(state_clash).contains(&DiagCode::DuplicateDefinition));
}

#[test]
fn reject_plain_children_on_a_component_reference() {
    // Rule 9 — these used to be DROPPED in silence at lowering.
    let src = r#"
Component Leaf {
    props: {
        label: Str,
    }
    Text($props.label)
}

Page P {
    Leaf { label: "x"
        Text("swallowed")
    }
}
"#;
    assert!(check_codes(src).contains(&DiagCode::SlotShape));
}

#[test]
fn reject_modifiers_on_a_component_reference() {
    // Rule 10 — also a silent drop before.
    let src = r#"
Component Leaf {
    props: {
        label: Str,
    }
    Text($props.label)
}

Page P {
    Leaf { label: "x" }
        .grow(1)
}
"#;
    assert!(check_codes(src).contains(&DiagCode::SlotShape));
}

#[test]
fn reject_slot_forwarding() {
    // Rule 11 — v1 has a slot FRAME, not a stack of them.
    let src = r#"
Component Inner {
    slot body
    Stack {
        Slot body
    }
}

Component Outer {
    slot body
    Stack {
        Inner { } {
            body {
                Slot body
            }
        }
    }
}

Page P {
    Outer { } { body { Text("x") } }
}
"#;
    assert!(check_codes(src).contains(&DiagCode::SlotShape));
}

#[test]
fn reject_two_placeholders_under_one_parent() {
    // Rule 12 — but NOT across the arms of an `if`, which `SCAFFOLD` proves.
    let src = r#"
Component Frame {
    slot content
    Stack {
        Slot content
        Slot content
    }
}

Page P {
    Frame { } { content { Text("x") } }
}
"#;
    assert!(check_codes(src).contains(&DiagCode::SlotShape));
}

#[test]
fn reject_callee_props_in_a_slot_body() {
    // Rule 13 — the body runs in the CALLER's frame, so `$props.contentPanel`
    // (a prop of the callee) has nothing to bind to here.
    let src = r#"
Component Frame {
    props: {
        contentPanel: Bool,
    }
    slot content
    Stack {
        Slot content
    }
}

Page P {
    Frame { contentPanel: true } {
        content {
            Text("x")
                .opacity($props.contentPanel)
        }
    }
}
"#;
    assert!(check_codes(src).contains(&DiagCode::UnknownField));
}

#[test]
fn reject_component_named_slot() {
    let src = r#"
Component Slot {
    props: {
        label: Str,
    }
    Text($props.label)
}

Page P {
    Text("x")
}
"#;
    assert!(check_codes(src).contains(&DiagCode::SlotShape));
}

// ------------------------------------------------------------- formatter

#[test]
fn formatter_round_trips_slot_syntax() {
    let src = page(
        r#"    Frame { contentPanel: true } {
        sidebar { Text("nav") }
        content { Panel { Text("a") } Panel { Text("b") } }
    }"#,
    );
    let file = parse_file(&src).expect("parses");
    let once = format_file(&file);
    let twice = format_file(&parse_file(&once).expect("reformats"));
    assert_eq!(once, twice, "format_file must be idempotent over slot syntax\n{once}");
    assert!(once.contains("slot sidebar"), "{once}");
    assert!(once.contains("Slot content"), "{once}");
    assert!(once.contains("content {"), "{once}");
}

#[test]
fn formatter_keeps_an_empty_prop_block_before_a_slot_block() {
    // `Frame { } { content { … } }` must not collapse into `Frame { content …`,
    // which would re-parse as a PROP block.
    let src = r#"
Component Frame {
    slot content
    Stack {
        Slot content
    }
}

Page P {
    Frame { } {
        content {
            Text("x")
        }
    }
}
"#;
    let once = format_file(&parse_file(src).expect("parses"));
    let twice = format_file(&parse_file(&once).expect("reformats"));
    assert_eq!(once, twice, "{once}");
}

// ------------------------------------------------------------ lowering (P1)

#[test]
fn lowering_rejects_slots_until_ir_v1_5() {
    // Phase 1 contract: slots parse and check, but lowering refuses loudly
    // rather than silently dropping the content regions.
    let src = page(r#"    Frame { contentPanel: true } { content { Text("x") } }"#);
    let file = parse_file(&src).expect("parses");
    let (model, diags) = check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "{diags:?}");
    let canonical = format_file(&file);
    match nexus_dsl_core::lower_file(&file, &model, &canonical) {
        Ok(_) => panic!("slots must not lower before IR v1.5"),
        Err(err) => assert_eq!(err.code, DiagCode::LoweringUnsupported),
    }
}

/// Tiny `.pipe` so the reject cases read as `source.pipe(check_codes)`.
trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(&str) -> T) -> T;
}

impl Pipe for String {
    fn pipe<T>(self, f: impl FnOnce(&str) -> T) -> T {
        f(&self)
    }
}
