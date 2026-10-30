// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0113 §3 / ADR-0090 — windowd's key-route gate on the host: the key channel takes
//! inputd's frames only, a key needs a focused window, the input method's pushes reach only the
//! holder they name, and a window without a field takes actions, never text.
//! OWNERS: @ui @runtime

use windowd::key_gate::{
    compose_session, imed_push_holder, imed_sender, key_sender, key_target, Holders, Refusal,
    FIELD_KIND_NONE,
};

const INPUTD: u64 = 0x11;
const IMED: u64 = 0x22;
const APP: u64 = 0x99;

#[test]
fn keys_from_inputd_reach_the_focused_window() {
    assert_eq!(key_sender(INPUTD, INPUTD), Ok(()));
    assert_eq!(key_target(Some(4u32)), Ok(4));
}

#[test]
fn test_reject_key_from_a_stranger() {
    assert_eq!(key_sender(APP, INPUTD), Err(Refusal::ForeignSender));
    assert_eq!(key_sender(IMED, INPUTD), Err(Refusal::ForeignSender), "imed sends no keys here");
    assert_eq!(key_sender(0, 0), Err(Refusal::ForeignSender), "an unnamed sender is nobody");
}

#[test]
fn test_reject_text_for_an_unfocused_surface() {
    assert_eq!(key_target::<u32>(None), Err(Refusal::NoWindowFocus));
    // The input method's text for a surface that no longer holds the field: refused.
    let holders = Holders { field: Some(7), window: None };
    assert_eq!(imed_push_holder(8, false, holders), Err(Refusal::StaleSurface));
    assert_eq!(imed_push_holder(7, false, holders), Ok(7));
}

#[test]
fn test_reject_imed_commit_from_a_stranger() {
    assert_eq!(imed_sender(IMED, IMED), Ok(()));
    assert_eq!(imed_sender(APP, IMED), Err(Refusal::ForeignSender));
    assert_eq!(imed_sender(INPUTD, IMED), Err(Refusal::ForeignSender));
    assert_eq!(imed_sender(0, 0), Err(Refusal::ForeignSender));
}

#[test]
fn test_reject_text_to_a_window_without_a_field() {
    let holders = Holders { field: None, window: Some(3) };
    assert_eq!(imed_push_holder(3, true, holders), Ok(3), "Escape reaches the window");
    assert_eq!(imed_push_holder(3, false, holders), Err(Refusal::TextWithoutField));
    assert_eq!(imed_push_holder(4, true, holders), Err(Refusal::StaleSurface));
    let nobody = Holders { field: None, window: None };
    assert_eq!(imed_push_holder(3, true, nobody), Err(Refusal::StaleSurface));
}

#[test]
fn a_composition_key_opens_the_fields_session_or_the_windows() {
    assert_eq!(compose_session(Some((7, 1)), Some(3)), Some((7, 1)));
    assert_eq!(compose_session(None, Some(3)), Some((3, FIELD_KIND_NONE)));
}
