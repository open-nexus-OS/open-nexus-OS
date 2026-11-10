// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0084 amendment (2026-11-10) at the checker: a slot body may place a slot of the component
//! it is written in — forwarding what that component received into the one it calls — and
//! nothing else: a slot the enclosing component does not declare, or any slot in a page's body,
//! is NX0411. The runtime half (the frames chain) is `userspace/dsl/runtime/tests/slots.rs`.

use nexus_dsl_core::{check_file, parse_file, DiagCode};

fn codes(src: &str) -> Vec<DiagCode> {
    let file = parse_file(src).expect("parses");
    check_file(&file).1.iter().map(|d| d.code).collect()
}

const CARD: &str = "Component Card {\n    slot body\n    Stack {\n        Slot body\n    }\n}\n";

#[test]
fn forwarding_a_slot_the_component_declares_checks_clean() {
    let src = format!(
        "{CARD}Component Pane {{\n    slot body\n    Stack {{\n        Card {{ }} {{\n            body {{\n                Slot body\n            }}\n        }}\n    }}\n}}\nPage P {{\n    Pane {{ }} {{}}\n}}\n"
    );
    assert!(codes(&src).is_empty(), "{:?}", codes(&src));
}

#[test]
fn test_reject_forwarding_a_slot_the_component_does_not_declare() {
    let src = format!(
        "{CARD}Component Pane {{\n    slot body\n    Stack {{\n        Card {{ }} {{\n            body {{\n                Slot footer\n            }}\n        }}\n    }}\n}}\nPage P {{\n    Pane {{ }} {{}}\n}}\n"
    );
    assert!(codes(&src).contains(&DiagCode::SlotShape), "{:?}", codes(&src));
}

#[test]
fn test_reject_a_slot_in_a_pages_slot_body() {
    let src = format!(
        "{CARD}Page P {{\n    Stack {{\n        Card {{ }} {{\n            body {{\n                Slot body\n            }}\n        }}\n    }}\n}}\n"
    );
    assert!(codes(&src).contains(&DiagCode::SlotShape), "{:?}", codes(&src));
}
