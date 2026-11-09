// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0117 at the frontend: `Window { width, height, minWidth, minHeight }` — the app's default
//! and minimum content size, bounded, never a default below its minimum, carried into the IR.

use nexus_dsl_core::{check_file, format_file, lower_file, parse_file, DiagCode};

const CALC: &str = "Window {\n    style: plain,\n    resizable: true,\n    width: 340,\n    height: 600,\n    minWidth: 340,\n    minHeight: 600,\n}\nPage P { Stack { } }\n";

#[test]
fn the_size_fields_parse_format_and_reach_the_ir() {
    let file = parse_file(CALC).expect("parses");
    let (model, diags) = check_file(&file);
    assert!(diags.is_empty(), "{diags:?}");
    let window = model.window.expect("a window intent");
    assert_eq!((window.width, window.height), (Some(340), Some(600)));
    assert_eq!((window.min_width, window.min_height), (Some(340), Some(600)));
    // The formatter keeps the fields (and is idempotent over them).
    let formatted = format_file(&file);
    assert!(
        formatted.contains("minWidth: 340,") && formatted.contains("height: 600,"),
        "{formatted}"
    );
    let again = format_file(&parse_file(&formatted).expect("formatted parses"));
    assert_eq!(formatted, again, "the formatter is idempotent");
    // The IR carries them (0 = absent).
    let lowered = lower_file(&file, &model, CALC).expect("lowers");
    let reader =
        nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(&lowered.nxir).expect("reads");
    let root = reader.root().expect("root");
    let w = root.get_window().expect("window");
    assert_eq!(
        (w.get_width(), w.get_height(), w.get_min_width(), w.get_min_height()),
        (340, 600, 340, 600)
    );
}

#[test]
fn an_absent_size_is_zero_in_the_ir() {
    let src = "Window { resizable: true, }\nPage P { Stack { } }\n";
    let file = parse_file(src).expect("parses");
    let (model, _) = check_file(&file);
    let lowered = lower_file(&file, &model, src).expect("lowers");
    let reader =
        nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(&lowered.nxir).expect("reads");
    let w = reader.root().expect("root").get_window().expect("window");
    assert_eq!(
        (w.get_width(), w.get_height(), w.get_min_width(), w.get_min_height()),
        (0, 0, 0, 0)
    );
}

#[test]
fn test_reject_a_default_below_the_minimum() {
    let err = parse_file("Window { width: 300, minWidth: 340, }\nPage P { Stack { } }\n")
        .expect_err("a default below its minimum is refused");
    assert_eq!(err.code, DiagCode::TypeMismatch);
    assert!(err.message.contains("below its minimum"), "{}", err.message);
}

#[test]
fn test_reject_a_window_size_out_of_range() {
    for bad in ["width: 0", "minHeight: 9000", "height: -4", "minWidth: wide"] {
        let src = format!("Window {{ {bad}, }}\nPage P {{ Stack {{ }} }}\n");
        assert!(parse_file(&src).is_err(), "`{bad}` must be refused");
    }
}
