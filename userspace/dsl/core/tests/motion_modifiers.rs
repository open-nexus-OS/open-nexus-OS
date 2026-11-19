// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0354 P10/P12/P13 at the checker: `.motion(duration, curve)` takes the theme's two closed
//! vocabularies, `.press(kind)` the press roles, `.rotate(deg)` turns only a glyph (NX0416), the
//! two new motion tokens validate, and the modifier ids stay append-only (they are positional in
//! the IR).

use nexus_dsl_core::{check_file, parse_file, registry, DiagCode};

fn codes(view: &str) -> Vec<DiagCode> {
    let src = format!("Store S {{ open: Bool = false, n: Int = 0, }}\nPage P {{\n{view}\n}}\n");
    let file = parse_file(&src).expect("parses");
    check_file(&file).1.iter().map(|d| d.code).collect()
}

#[test]
fn a_motion_takes_a_duration_and_a_curve() {
    let ok = "    Stack { }\n        .width(8)\n        .motion(quick, springSoft)";
    assert!(codes(ok).is_empty(), "{:?}", codes(ok));
    for d in registry::MOTION_DURATIONS {
        for c in registry::MOTION_CURVES {
            let v = format!("    Stack {{ }}\n        .width(8)\n        .motion({d}, {c})");
            assert!(codes(&v).is_empty(), "{d} {c}: {:?}", codes(&v));
        }
    }
}

#[test]
fn test_reject_a_motion_duration_outside_the_vocabulary() {
    let bad = "    Stack { }\n        .width(8)\n        .motion(fast, spring)";
    assert!(codes(bad).contains(&DiagCode::UnknownName), "{:?}", codes(bad));
}

#[test]
fn test_reject_a_motion_curve_outside_the_vocabulary() {
    let bad = "    Stack { }\n        .width(8)\n        .motion(quick, bounce)";
    assert!(codes(bad).contains(&DiagCode::UnknownName), "{:?}", codes(bad));
    let swapped = "    Stack { }\n        .width(8)\n        .motion(spring, quick)";
    assert!(codes(swapped).contains(&DiagCode::UnknownName), "{:?}", codes(swapped));
}

#[test]
fn a_rotation_turns_an_icon() {
    let ok = "    Icon { symbol: \"chevron.down\", size: 12 }\n        .rotate(180)\n        .motion(quick, springSoft)";
    assert!(codes(ok).is_empty(), "{:?}", codes(ok));
}

#[test]
fn test_reject_rotate_on_a_container() {
    let bad =
        "    Stack {\n        Icon { symbol: \"chevron.down\", size: 12 }\n    }\n    .rotate(180)";
    assert!(codes(bad).contains(&DiagCode::RotateTarget), "{:?}", codes(bad));
    assert_eq!(DiagCode::RotateTarget.code(), "NX0416");
}

#[test]
fn test_reject_an_unknown_press_kind() {
    let bad = "    Stack { }\n        .width(8)\n        .press(squish)";
    assert!(codes(bad).contains(&DiagCode::UnknownName), "{:?}", codes(bad));
    for k in registry::PRESS_KINDS {
        let ok = format!("    Stack {{ }}\n        .width(8)\n        .press({k})");
        assert!(codes(&ok).is_empty(), "{k}: {:?}", codes(&ok));
    }
}

#[test]
fn the_new_motion_tokens_validate() {
    assert!(registry::is_motion_token("pop") && registry::is_motion_token("swapInline"));
    let ok = "    Stack { }\n        .width(8)\n        .animate(pop, value: state.open)";
    assert!(codes(ok).is_empty(), "{:?}", codes(ok));
}

/// Modifier ids are positional in the IR: a new modifier is appended, never inserted.
#[test]
fn modifier_ids_are_append_only() {
    let id = |n: &str| registry::modifier_spec(n).map(|(i, _)| i);
    assert_eq!(id("figures"), Some(62));
    assert_eq!(id("rotate"), Some(63));
    assert_eq!(id("motion"), Some(64));
    assert_eq!(id("press"), Some(65));
    assert_eq!(registry::MODIFIERS.len(), 66);
}
