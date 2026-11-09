// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0116 at the checker: the DSL names only the one token set. An old name is a compile
//! error that names its replacement, a spacing is on the 4-pt grid, a text size is one of the
//! twelve styles, and only one surface in a stack blurs.

use nexus_dsl_core::{check_file, parse_file, DiagCode, Diagnostic};

fn diags_of(src: &str) -> Vec<Diagnostic> {
    let file = parse_file(src).expect("parses");
    check_file(&file).1
}

fn message_of(src: &str, code: DiagCode) -> String {
    let diags = diags_of(src);
    diags
        .iter()
        .find(|d| d.code == code)
        .map(|d| d.message.clone())
        .unwrap_or_else(|| panic!("no {code:?} for {src}: {diags:?}"))
}

fn clean(src: &str) {
    let diags = diags_of(src);
    assert!(diags.is_empty(), "expected clean, got {diags:?} for {src}");
}

#[test]
fn test_reject_an_old_token_name() {
    let m = message_of("Page P { Text(\"x\").fg(onSurface) }", DiagCode::UnknownName);
    assert!(m.contains("retired") && m.contains("`label`"), "{m}");
    let m = message_of("Page P { Text(\"x\").fg(onGlassMuted) }", DiagCode::UnknownName);
    assert!(m.contains("`labelSecondary`"), "{m}");
    let m = message_of("Page P { Stack { }.bg(accentSoft) }", DiagCode::UnknownName);
    assert!(m.contains(".tint(accent)"), "{m}");
    let m = message_of("Page P { Stack { }.material(windowPane) }", DiagCode::UnknownName);
    assert!(m.contains("`pane`"), "{m}");
    let m = message_of("Page P { Stack { }.shadow(xxl) }", DiagCode::UnknownName);
    assert!(m.contains("`shadow4`"), "{m}");
    let m = message_of("Page P { Stack { }.border(thin) }", DiagCode::UnknownName);
    assert!(m.contains("`hairline`"), "{m}");
    let m = message_of("Page P { Text(\"x\").fontWeight(regular) }", DiagCode::UnknownName);
    assert!(m.contains("`normal`"), "{m}");
    // A name that never existed still gets the near-miss or the list.
    let m = message_of("Page P { Text(\"x\").fg(Label) }", DiagCode::UnknownName);
    assert!(m.contains("did you mean `label`"), "{m}");
    // The set itself checks clean.
    clean(
        "Page P { Stack { Text(\"x\").fg(labelSecondary).textStyle(footnote).fontWeight(semibold) }\
         .bg(fill).rounded(md).shadow(shadow3, edgeHighlight).border(hairline)\
         .borderColor(separator).padding(12).gap(gapInline) }",
    );
}

#[test]
fn test_reject_an_off_grid_spacing() {
    let m = message_of("Page P { Stack { }.padding(7) }", DiagCode::UnknownName);
    assert!(m.contains("4-pt grid"), "{m}");
    assert!(diags_of("Page P { Stack { }.gap(10) }")
        .iter()
        .any(|d| d.code == DiagCode::UnknownName));
    assert!(diags_of("Page P { Stack { }.gap(1.5) }")
        .iter()
        .any(|d| d.code == DiagCode::UnknownName));
    assert!(diags_of("Page P { Stack { }.padding(huge) }")
        .iter()
        .any(|d| d.code == DiagCode::UnknownName));
    clean("Page P { Stack { }.gap(6).padding(marginLayout).paddingX(2).hitSlop(8) }");
    clean("Page P { Stack { }.gap(gapSection).paddingY(48) }");
}

#[test]
fn test_reject_a_text_size_outside_the_styles() {
    let m = message_of("Page P { Text(\"x\").textSize(lg) }", DiagCode::UnknownModifier);
    assert!(m.contains("retired") && m.contains(".textStyle"), "{m}");
    let m = message_of("Page P { Text(\"x\").leading(tight) }", DiagCode::UnknownModifier);
    assert!(m.contains("line height"), "{m}");
    let m = message_of("Page P { Text(\"x\").textShadow(soft) }", DiagCode::UnknownModifier);
    assert!(m.contains(".shadow(halo)"), "{m}");
    assert!(diags_of("Page P { Text(\"x\").textStyle(huge) }")
        .iter()
        .any(|d| d.code == DiagCode::UnknownName));
    for style in nexus_dsl_core::registry::TEXT_STYLES {
        clean(&format!("Page P {{ Text(\"x\").textStyle({style}) }}"));
    }
}

#[test]
fn test_reject_a_material_inside_a_material() {
    let nested = "Page P { Stack { Stack { }.material(thin) }.material(panel) }";
    assert!(diags_of(nested).iter().any(|d| d.code == DiagCode::NestedBlur));
    let in_panel_widget = "Page P { Panel { Stack { }.material(window) } }";
    assert!(diags_of(in_panel_widget).iter().any(|d| d.code == DiagCode::NestedBlur));
    let deep = "Page P { Stack { Stack { Stack { }.material(dock) } }.material(ultraThin) }";
    assert!(diags_of(deep).iter().any(|d| d.code == DiagCode::NestedBlur));
    // A layer on a blur surface is a card or opaque; siblings each blur on their own.
    clean(
        "Page P { Stack { Stack { }.material(card) Stack { }.material(overlay) }.material(panel) }",
    );
    clean("Page P { Panel { Stack { }.material(pane) } }");
    clean("Page P { Stack { Stack { }.material(panel) Stack { }.material(dock) } }");
}

#[test]
fn tint_takes_a_role_and_a_strength() {
    clean("Page P { Stack { }.tint(accent) }");
    clean("Page P { Stack { }.tint(onAccent, border).borderTint(accent) }");
    clean("Page P { Stack { }.tint(success, line).corners(lg, lg, sm, 16) }");
    assert!(diags_of("Page P { Stack { }.tint(accent, strong) }")
        .iter()
        .any(|d| d.code == DiagCode::UnknownName));
    let m = message_of("Page P { Stack { }.tint(info) }", DiagCode::UnknownName);
    assert!(m.contains("`accent`"), "{m}");
}

#[test]
fn a_colour_role_from_a_string_is_checked() {
    clean("Page P { Stack { }.bg(\"label\") }");
    let m = message_of("Page P { Stack { }.bg(\"onGlass\") }", DiagCode::UnknownName);
    assert!(m.contains("`label`"), "{m}");
}
