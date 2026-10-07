// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A scene site that reads state only THROUGH a list operation must re-emit when that state
//! changes. The dependency walk once skipped list operations, so `if len($state.items) == 0`
//! kept showing "empty" after an effect filled the list, and `List(take($state.items, n))`
//! never grew: only the list changed, and nothing on screen was known to read it. The shell
//! search's clipboard panel hit exactly this after a copy (TASK-0067B board round).

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use nexus_dsl_runtime::{Damage, FixtureEnv, IdentityLocale, NoIo, Value, View};

fn texts(v: &View<'_>) -> Vec<String> {
    fn walk(n: &nexus_layout_types::LayoutNode, out: &mut Vec<String>) {
        match n {
            nexus_layout_types::LayoutNode::Stack(_, _, c)
            | nexus_layout_types::LayoutNode::Grid(_, _, c) => {
                for ch in c {
                    walk(ch, out);
                }
            }
            nexus_layout_types::LayoutNode::Text(t, _) => {
                out.push(String::from(t.content.as_str()))
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v.scene(), &mut out);
    out
}

/// Mounts `page`, dispatches each `(case, payload)` in order and returns, per step, the damage
/// and the texts after it (the first entry is the mount: `Damage::None` and the initial texts).
fn run(page: &str, steps: Vec<(&str, Vec<Value>)>) -> Vec<(Damage, Vec<String>)> {
    let file = nexus_dsl_core::parse_file(page).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    let canonical = nexus_dsl_core::format_file(&file);
    let nxir = nexus_dsl_core::lower_file(&file, &model, &canonical).expect("lowers").nxir;
    let (tokens, device) = (nexus_theme_tokens::BaseTokens, FixtureEnv::default());
    let (symbols, keys): (Vec<String>, Vec<u32>) = (Vec::new(), Vec::new());
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut out = vec![(Damage::None, texts(&view))];
    for (case, payload) in steps {
        let (e, c) = view.runtime().event_case("E", case).expect("case");
        let damage =
            view.dispatch(&tokens, &device, &locale, &mut NoIo, e, c, payload).expect("runs");
        out.push((damage, texts(&view)));
    }
    out
}

fn strs(items: &[&str]) -> Vec<Value> {
    vec![Value::List(items.iter().map(|s| Value::Str((*s).to_string())).collect())]
}

const STORE: &str = r#"Store S {
    items: List<Str> = [],
    limit: Int = 1,
}

Event E {
    Loaded(List<Str>),
    Widen,
}

reduce E {
    Loaded(items) => state.items = items,
    Widen => state.limit = 3,
}
"#;

#[test]
fn a_len_guard_re_emits_when_only_the_list_changes() {
    let page = format!(
        "{STORE}\nPage P {{\n    Stack {{\n        if len($state.items) == 0 {{\n            \
         Text(\"empty\")\n        }} else {{\n            Text(\"filled\")\n        }}\n    }}\n}}\n"
    );
    let steps = run(&page, vec![("Loaded", strs(&["a"]))]);
    assert_eq!(steps[0].1, ["empty"]);
    assert_ne!(steps[1].0, Damage::None, "the guard reads the list");
    assert_eq!(steps[1].1, ["filled"], "the branch follows the list");
}

#[test]
fn a_sliced_list_re_emits_when_its_list_or_its_count_changes() {
    let page = format!(
        "{STORE}\nPage P {{\n    Stack {{\n        List(take(skip($state.items, 0), $state.limit)) \
         {{ x in\n            Stack {{\n                Text(x)\n            }}\n            \
         .key(x)\n        }}\n    }}\n}}\n"
    );
    let steps = run(&page, vec![("Loaded", strs(&["a", "b", "c"])), ("Widen", vec![])]);
    assert!(steps[0].1.is_empty());
    assert_ne!(steps[1].0, Damage::None, "take(skip(list)) reads the list");
    assert_eq!(steps[1].1, ["a"], "the slice shows `limit` items");
    assert_ne!(steps[2].0, Damage::None, "take's count reads `limit`");
    assert_eq!(steps[2].1, ["a", "b", "c"], "a wider count shows more");
}

#[test]
fn test_reject_damage_for_a_list_nothing_on_screen_reads() {
    let page = format!("{STORE}\nPage P {{\n    Stack {{\n        Text(\"static\")\n    }}\n}}\n");
    let steps = run(&page, vec![("Loaded", strs(&["a"]))]);
    assert_eq!(steps[1].0, Damage::None, "no site reads the list — no re-emit");
    assert_eq!(steps[1].1, ["static"]);
}
