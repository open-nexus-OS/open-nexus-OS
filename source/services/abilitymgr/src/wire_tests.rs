// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The wire dispatch tests of the lifecycle authority (`wire.rs`): launches, kept
//! singletons, denials, the per-app limit (no table bound), deaths and relaunches — split out of
//! `wire.rs` (structure-gate, TASK-0352 P0). `super` is `wire`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use super::*;

/// The grants the tests' authority publishes: every app of the matrix holds WINDOW, like
/// the real manifests; `ghost` is never published (RFC-0111 §6: no grants, no launch).
pub(super) const TEST_GRANTS: &[(&str, &[&str])] = &[
    ("search", &["nexus.permission.WINDOW"]),
    ("notes", &["nexus.permission.WINDOW"]),
    ("chat", &["nexus.permission.WINDOW", "nexus.permission.CLIPBOARD"]),
    ("desktop-shell", &["nexus.permission.WINDOW"]),
];

/// `super::dispatch` against the tests' authority.
pub(super) fn dispatch(broker: &mut Broker, frame: &[u8]) -> Dispatched {
    super::dispatch(broker, frame, &mut StaticGrants(TEST_GRANTS))
}

/// TASK-0352 P1/P3 (ADR-0093): the table has no constant — the 33rd instance launches — and an
/// app at its own limit is refused by name (`STATUS_FULL`, the event naming app, instances and
/// limit) while another app still launches.
#[test]
fn test_reject_a_launch_past_the_app_s_limit_by_name_while_others_launch() {
    use crate::lifecycle::DEFAULT_INSTANCES_PER_APP as LIMIT;
    let mut broker = Broker::new();
    for n in 0..LIMIT {
        let d = dispatch(&mut broker, &encode_launch("notes", "notes.main"));
        assert_eq!(resp_op_status(&d.response), (OP_LAUNCH | OP_RESPONSE, STATUS_OK), "{n}");
        broker.bind_pid(launched_id(&d), 100 + n as u32).expect("bind");
    }
    assert!(LIMIT > 32, "the 33rd instance launched: no table constant");
    let full = dispatch(&mut broker, &encode_launch("notes", "notes.main"));
    assert_eq!(resp_op_status(&full.response), (OP_LAUNCH | OP_RESPONSE, STATUS_FULL));
    assert_eq!(
        full.event,
        Some(Event::LaunchRefused { app_id: "notes".into(), instances: LIMIT, limit: LIMIT })
    );
    assert_eq!(broker.len(), LIMIT, "nothing spawned");
    let other = dispatch(&mut broker, &encode_launch("search", "search.main"));
    assert_eq!(resp_op_status(&other.response), (OP_LAUNCH | OP_RESPONSE, STATUS_OK));
    assert_eq!(broker.len(), LIMIT + 1, "the limit is the app's, not the table's");
}

#[test]
fn test_reject_launch_without_published_grants() {
    // An app the authority holds nothing for — never installed, or its manifest was
    // rejected at publish — is DENIED, and the denial is an event (a marker on the OS).
    let mut broker = Broker::new();
    let d = dispatch(&mut broker, &encode_launch("ghost", "ghost.main"));
    assert_eq!(resp_op_status(&d.response), (OP_LAUNCH | OP_RESPONSE, STATUS_DENIED));
    assert_eq!(d.event, Some(Event::LaunchDenied { app_id: "ghost".into() }));
    assert_eq!(broker.len(), 0, "nothing was created");
    // The authority's answer IS the granted set recorded on the instance.
    let d = dispatch(&mut broker, &encode_launch("chat", "chat.main"));
    let id = u32::from_le_bytes([d.response[5], d.response[6], d.response[7], d.response[8]]);
    assert_eq!(
        broker.instance(id).map(|i| i.granted_caps.as_slice()),
        Some(&["nexus.permission.WINDOW".into(), "nexus.permission.CLIPBOARD".into()][..])
    );
}

fn resp_op_status(frame: &[u8]) -> (u8, u8) {
    (frame[3], frame[4])
}

#[test]
fn launch_then_transition_roundtrip() {
    let mut broker = Broker::new();
    let d = dispatch(&mut broker, &encode_launch("search", "search.main"));
    let (op, status) = resp_op_status(&d.response);
    assert_eq!(op, OP_LAUNCH | OP_RESPONSE);
    assert_eq!(status, STATUS_OK);
    let id = u32::from_le_bytes([d.response[5], d.response[6], d.response[7], d.response[8]]);
    assert_eq!(d.event, Some(Event::Launched { app_id: "search".into(), instance_id: id }));

    // Created → Started.
    let d = dispatch(&mut broker, &encode_transition(id, AbilityState::Started));
    assert_eq!(resp_op_status(&d.response), (OP_TRANSITION | OP_RESPONSE, STATUS_OK));
    assert_eq!(d.event, Some(Event::Transitioned { instance_id: id, to: AbilityState::Started }));
}

#[test]
fn invalid_transition_status() {
    let mut broker = Broker::new();
    let d = dispatch(&mut broker, &encode_launch("notes", "notes.main"));
    let id = u32::from_le_bytes([d.response[5], d.response[6], d.response[7], d.response[8]]);
    // Created → Foreground is illegal (must Start first).
    let d = dispatch(&mut broker, &encode_transition(id, AbilityState::Foreground));
    assert_eq!(resp_op_status(&d.response).1, STATUS_INVALID_TRANSITION);
    assert!(d.event.is_none());
}

#[test]
fn unknown_instance_status() {
    let mut broker = Broker::new();
    let d = dispatch(&mut broker, &encode_transition(42, AbilityState::Started));
    assert_eq!(resp_op_status(&d.response).1, STATUS_UNKNOWN);
}

#[test]
fn malformed_frames_rejected() {
    let mut broker = Broker::new();
    assert_eq!(resp_op_status(&dispatch(&mut broker, &[]).response).1, STATUS_MALFORMED);
    assert_eq!(
        resp_op_status(&dispatch(&mut broker, &[b'X', b'Y', 1, OP_LAUNCH]).response).1,
        STATUS_MALFORMED
    );
    // Bad version.
    assert_eq!(
        resp_op_status(&dispatch(&mut broker, &[MAGIC0, MAGIC1, 99, OP_LAUNCH]).response).1,
        STATUS_MALFORMED
    );
}

#[test]
fn a_death_report_removes_the_instance_and_answers_nothing() {
    let mut broker = Broker::new();
    let d = dispatch(&mut broker, &encode_launch("chat", "chat.main"));
    let id = u32::from_le_bytes([d.response[5], d.response[6], d.response[7], d.response[8]]);
    broker.bind_pid(id, 77).unwrap();
    let d = dispatch(&mut broker, &encode_instance_exited(77, 0, 0));
    assert!(d.response.is_empty(), "an event, never a request");
    assert_eq!(
        d.event,
        Some(Event::Exited { instance_id: id, app_id: "chat".into(), pid: 77, code: 0, live: 0 })
    );
    assert!(broker.is_empty());
    // The same report again names a pid no instance claims.
    let d = dispatch(&mut broker, &encode_instance_exited(77, 0, 0));
    assert_eq!(d.event, Some(Event::ExitedUnknown { pid: 77 }));
    assert!(d.response.is_empty());
}

#[test]
fn test_reject_death_report_from_anyone_but_execd() {
    let frame = encode_instance_exited(77, 0, 0);
    let (execd, other) = (0x1111u64, 0x2222u64);
    assert!(exited_frame_allowed(&frame, execd, execd));
    assert!(!exited_frame_allowed(&frame, other, execd), "a forged death is refused");
    // Every other op is sender-agnostic here (its own gates apply).
    assert!(exited_frame_allowed(&encode_launch("chat", "chat.main"), other, execd));
    // A malformed report is answered as malformed, not applied.
    let mut broker = Broker::new();
    let d = dispatch(&mut broker, &frame[..12]);
    assert_eq!(resp_op_status(&d.response), (OP_INSTANCE_EXITED | OP_RESPONSE, STATUS_MALFORMED));
}

#[test]
fn recents_count_reported() {
    let mut broker = Broker::new();
    dispatch(&mut broker, &encode_launch("search", "s.main"));
    dispatch(&mut broker, &encode_launch("chat", "c.main"));
    let d = dispatch(&mut broker, &[MAGIC0, MAGIC1, VERSION, OP_RECENTS]);
    assert_eq!(resp_op_status(&d.response), (OP_RECENTS | OP_RESPONSE, STATUS_OK));
    let count = u16::from_le_bytes([d.response[5], d.response[6]]);
    assert_eq!(count, 2);
}

fn launched_id(d: &Dispatched) -> u32 {
    u32::from_le_bytes([d.response[5], d.response[6], d.response[7], d.response[8]])
}

#[test]
fn a_one_instance_role_asked_again_answers_the_running_instance() {
    let mut broker = Broker::new();
    let first = dispatch(&mut broker, &encode_launch("desktop-shell", "main"));
    let id = launched_id(&first);
    assert!(matches!(first.event, Some(Event::Launched { .. })));
    // Spawned: the OS loop bound its process.
    broker.bind_pid(id, 40).expect("bind");
    let again = dispatch(&mut broker, &encode_launch("desktop-shell", "main"));
    assert_eq!(resp_op_status(&again.response), (OP_LAUNCH | OP_RESPONSE, STATUS_OK));
    assert_eq!(launched_id(&again), id, "the running instance answers");
    assert_eq!(
        again.event,
        Some(Event::LaunchKept { app_id: "desktop-shell".into(), instance_id: id })
    );
    // TASK-0348 P12: a kept instance runs — its reply never says `Created`, so the asker's
    // wait ring ends on it instead of running out to its failsafe.
    assert_eq!(again.response[9], AbilityState::Started.as_wire(), "state on the wire");
    assert_eq!(broker.state(id), Some(AbilityState::Started));
    assert_eq!(broker.len(), 1, "nothing spawned");
    // Gone, it launches anew.
    broker.exited(40).expect("exited");
    let anew = dispatch(&mut broker, &encode_launch("desktop-shell", "main"));
    assert!(matches!(anew.event, Some(Event::Launched { .. })));
}

/// RFC-0112 (TASK-0348 P15): the shell died during the restart, and its death report came
/// after the new compositor's launch was answered with it — nobody asks again, so the broker
/// owes the role a launch; once the compositor has SEEN the kept window, a death is the
/// instance's own and owes nothing.
#[test]
fn a_kept_role_that_dies_before_it_is_seen_is_owed_a_launch() {
    let mut broker = Broker::new();
    let id = launched_id(&dispatch(&mut broker, &encode_launch("desktop-shell", "main")));
    broker.bind_pid(id, 40).expect("bind");
    let kept = dispatch(&mut broker, &encode_launch("desktop-shell", "main"));
    assert!(matches!(kept.event, Some(Event::LaunchKept { .. })));
    let died = dispatch(&mut broker, &encode_instance_exited(40, -11, 2));
    assert!(matches!(died.event, Some(Event::Exited { .. })));
    assert_eq!(broker.take_relaunch().as_deref(), Some("desktop-shell"));
    assert_eq!(broker.take_relaunch(), None, "owed once");
    let anew = dispatch(&mut broker, &encode_launch("desktop-shell", "main"));
    assert!(matches!(anew.event, Some(Event::Launched { .. })));
    assert_eq!(broker.len(), 1);
}

#[test]
fn test_reject_a_relaunch_for_a_seen_kept_role_or_an_ordinary_app() {
    use crate::seen::Seen;
    let mut broker = Broker::new();
    let id = launched_id(&dispatch(&mut broker, &encode_launch("desktop-shell", "main")));
    broker.bind_pid(id, 40).expect("bind");
    dispatch(&mut broker, &encode_launch("desktop-shell", "main"));
    broker.seen(id, Seen::Focused, 1_000).expect("seen");
    dispatch(&mut broker, &encode_instance_exited(40, 0, 0));
    assert_eq!(broker.take_relaunch(), None, "seen: its death is its own");
    // An ordinary app is never kept, so its death never owes a launch.
    let a = launched_id(&dispatch(&mut broker, &encode_launch("notes", "main")));
    broker.bind_pid(a, 41).expect("bind");
    dispatch(&mut broker, &encode_launch("notes", "main"));
    dispatch(&mut broker, &encode_instance_exited(41, -11, 2));
    assert_eq!(broker.take_relaunch(), None);
}

#[test]
fn test_reject_a_kept_launch_for_an_ordinary_app_or_an_unspawned_role() {
    let mut broker = Broker::new();
    // An ordinary app opens as many windows as it is launched.
    let a = launched_id(&dispatch(&mut broker, &encode_launch("notes", "main")));
    broker.bind_pid(a, 41).expect("bind");
    let b = dispatch(&mut broker, &encode_launch("notes", "main"));
    assert!(matches!(b.event, Some(Event::Launched { .. })));
    assert_ne!(launched_id(&b), a);
    // A role whose spawn never happened (no process) is no running instance.
    let mut broker = Broker::new();
    dispatch(&mut broker, &encode_launch("desktop-shell", "main"));
    let again = dispatch(&mut broker, &encode_launch("desktop-shell", "main"));
    assert!(matches!(again.event, Some(Event::Launched { .. })));
}
