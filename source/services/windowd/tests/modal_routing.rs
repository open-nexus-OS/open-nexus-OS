// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0074 D4 — windowd's app-modal routing gate on the host: while one window of
//! an owner is modal, the owner's other windows take no input; the modal itself, other owners
//! and free slots are untouched; the gate lifts with the flag. Drives the pure gate the press,
//! hover and wheel loops consult (`modal_gate`).
//! OWNERS: @ui

use windowd::modal_gate::{input_verdict, SlotFacts, Verdict};

const fn slot(owner_sid: u64, modal: bool, live: bool) -> SlotFacts {
    SlotFacts { owner_sid, modal, live }
}

/// Two windows of app 7, one modal: the sibling is refused, the modal and app 9 pass; once the
/// modal is off, the sibling passes again.
#[test]
fn an_app_modal_refuses_the_owners_other_windows_until_it_is_off() {
    let mut slots =
        [slot(7, true, true), slot(7, false, true), slot(9, false, true), slot(0, false, false)];
    assert_eq!(input_verdict(1, &slots), Verdict::RefusedByModal { modal_idx: 0 });
    assert_eq!(input_verdict(0, &slots), Verdict::Allow);
    assert_eq!(input_verdict(2, &slots), Verdict::Allow);
    assert_eq!(input_verdict(3, &slots), Verdict::Allow);
    slots[0].modal = false;
    assert_eq!(input_verdict(1, &slots), Verdict::Allow);
}

/// Two modals of the same owner: each refuses the other (the DSL runtime never opens two
/// app-modal windows of one app at once; the gate stays fail-closed if it did).
#[test]
fn test_reject_two_modals_of_one_owner_block_each_other() {
    let slots = [slot(7, true, true), slot(7, true, true)];
    assert_eq!(input_verdict(0, &slots), Verdict::Allow);
    assert_eq!(input_verdict(1, &slots), Verdict::Allow);
    let with_plain = [slot(7, true, true), slot(7, true, true), slot(7, false, true)];
    assert_eq!(input_verdict(2, &with_plain), Verdict::RefusedByModal { modal_idx: 0 });
}
