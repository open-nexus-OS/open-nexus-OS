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
