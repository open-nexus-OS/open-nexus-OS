// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0357 P3 at the checker (NX0415): every tap target has an accessible name — a container
//! with `on Tap` carries `.label(…)` or holds something a reader can speak (a `Text`, a widget's
//! label prop, a component, a slot). The interactive widgets keep their own rule (NX0402).

use nexus_dsl_core::{check_file, parse_file, DiagCode};

fn codes(view: &str) -> Vec<DiagCode> {
    let src = format!("Event E {{ Go, }}\nStore S {{ n: Int = 0, }}\nreduce E {{ Go => state.n += 1, }}\nPage P {{\n{view}\n}}\n");
    let file = parse_file(&src).expect("parses");
    check_file(&file).1.iter().map(|d| d.code).collect()
}

#[test]
fn test_reject_a_tappable_container_without_a_name() {
    let icon_only = "    Circle { size: 28\n        Icon { symbol: \"xmark\", size: 12 }\n    }\n    on Tap -> dispatch(Go)";
    assert!(codes(icon_only).contains(&DiagCode::UnnamedTapTarget), "{:?}", codes(icon_only));
    let bare = "    Stack { }\n        .width(8)\n    on Tap -> dispatch(Go)";
    assert!(codes(bare).contains(&DiagCode::UnnamedTapTarget));
}

#[test]
fn a_label_or_a_text_names_a_tap_target() {
    let labelled = "    Circle { size: 28\n        Icon { symbol: \"xmark\", size: 12 }\n    }\n    .label(\"Close\")\n    on Tap -> dispatch(Go)";
    assert!(codes(labelled).is_empty(), "{:?}", codes(labelled));
    let texted = "    Stack {\n        Text(\"Cancel\")\n    }\n    on Tap -> dispatch(Go)";
    assert!(codes(texted).is_empty(), "{:?}", codes(texted));
}
