// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: imed contract tests (RFC-0075): focus gating, composition
//! outcomes and reject paths of the IME core exercised through the crate's
//! public API — the service-layout proof surface (`tests/*.rs`).
//! OWNERS: @ui
//! STATUS: Functional
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

use imed::ImedCore;
use nexus_wire::imed as wire;

#[test]
fn focus_gates_key_processing_end_to_end() {
    let mut core = ImedCore::new();
    // Unfocused: dropped (imed is the gate; inputd forwards unconditionally).
    assert_eq!(core.key(wire::KEY_KIND_TEXT, u32::from('a'), 0), None);
    core.set_focus(3, true, wire::FIELD_KIND_TEXT);
    let push = core.key(wire::KEY_KIND_TEXT, u32::from('a'), 0).expect("push");
    assert_eq!(push.surface_id, 3);
    assert_eq!(push.commit.expect("commit").as_str(), "a");
    // Unfocus drops keys again.
    core.set_focus(3, false, wire::FIELD_KIND_TEXT);
    assert_eq!(core.key(wire::KEY_KIND_TEXT, u32::from('b'), 0), None);
}

#[test]
fn de_dead_key_composition_round_trip() {
    let mut core = ImedCore::new();
    core.set_focus(1, true, wire::FIELD_KIND_TEXT);
    assert_eq!(core.key(wire::KEY_KIND_DEAD, u32::from('´'), 0), None);
    let push = core.key(wire::KEY_KIND_TEXT, u32::from('e'), 0).expect("push");
    assert_eq!(push.commit.expect("commit").as_str(), "é");
}

#[test]
fn test_reject_invalid_key_payloads() {
    let mut core = ImedCore::new();
    core.set_focus(1, true, wire::FIELD_KIND_TEXT);
    assert_eq!(core.key(0xFF, u32::from('a'), 0), None, "unknown kind");
    assert_eq!(core.key(wire::KEY_KIND_TEXT, 0xD800, 0), None, "surrogate scalar");
    assert_eq!(core.key(wire::KEY_KIND_ACTION, 0, 0xFF), None, "unknown action");
}

#[test]
fn test_reject_wire_frames_fail_closed() {
    // The server's frame vocabulary: every decoder is fail-closed (golden +
    // reject matrices live in nexus-wire; this pins the crate wiring).
    assert_eq!(wire::decode_key(&[b'I', b'E', 1, wire::OP_KEY, 0]), None);
    assert_eq!(wire::decode_set_focus(&[b'X', b'E', 1, wire::OP_SET_FOCUS]), None);
    let good = wire::encode_key(wire::KEY_SOURCE_HW, wire::KEY_KIND_TEXT, 97, 0, 0);
    assert!(wire::decode_key(&good).is_some());
    assert_eq!(wire::decode_key(&good[..good.len() - 1]), None);
}
