// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for dsoftbusd session orchestration steps and FSM interaction.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 5 tests
//!
//! TEST_SCOPE:
//!   - Reconnect path (session close + epoch bump)
//!   - Identity binding mismatch rejection
//!   - Absent identity binding (non-fatal)
//!   - Discovery step cadence rules (announce/poll intervals)
//!   - FSM phase setter coverage
//!
//! TEST_SCENARIOS:
//!   - test_reconnect_path_closes_old_session_and_advances_retry_state(): Handshake failure triggers reconnect with SID close and epoch advance
//!   - test_reject_identity_binding_mismatch(): Mismatched Noise static keys are rejected
//!   - test_identity_binding_absent_mapping_is_nonfatal(): Missing discovery mapping passes through
//!   - test_discovery_step_cadence_rules(): Announce and poll cadence bitmask rules are correct
//!   - test_fsm_phase_setters_are_exercised(): SessionFsm phase transitions work end-to-end
//!
//! DEPENDENCIES:
//!   - ../src/os/session/fsm.rs (via #[path])
//!   - ../src/os/session/steps.rs (via #[path])
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md
#[path = "../src/os/session/fsm.rs"]
mod fsm;
#[path = "../src/os/session/steps.rs"]
mod steps;

#[test]
fn test_reconnect_path_closes_old_session_and_advances_retry_state() {
    let mut f: fsm::SessionFsm<u32> = fsm::SessionFsm::new();
    f.set_connected(99);
    assert_eq!(f.epoch_raw(), 1);

    let action = steps::on_handshake_failure(&mut f);
    assert_eq!(action.close_sid, Some(99));
    assert!(action.retry);
    assert_eq!(f.sid(), None);
    assert_eq!(f.phase(), fsm::SessionPhase::Reconnect);
    assert_eq!(f.epoch_raw(), 2);
}

#[test]
fn test_reject_identity_binding_mismatch() {
    let expected = [0x11; 32];
    let discovered = Some([0x22; 32]);
    assert!(!steps::identity_binding_matches(discovered, expected));
}

#[test]
fn test_identity_binding_absent_mapping_is_nonfatal() {
    let expected = [0x11; 32];
    assert!(steps::identity_binding_matches(None, expected));
}

#[test]
fn test_discovery_step_cadence_rules() {
    assert!(steps::should_send_announce(false, 1));
    assert!(steps::should_send_announce(true, 64));
    assert!(!steps::should_send_announce(true, 65));

    assert!(steps::should_poll_discovery(false, 7));
    assert!(steps::should_poll_discovery(true, 32));
    assert!(!steps::should_poll_discovery(true, 33));
}

#[test]
fn test_fsm_phase_setters_are_exercised() {
    let mut f: fsm::SessionFsm<u32> = fsm::SessionFsm::new();
    f.set_listening();
    f.set_dialing();
    f.set_accepting();
    f.set_connected(1);
    f.set_handshaking();
    f.set_ready();
    assert_eq!(f.phase(), fsm::SessionPhase::Ready);
}
