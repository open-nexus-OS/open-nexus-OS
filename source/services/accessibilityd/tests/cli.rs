//! CONTEXT: Accessibility daemon CLI tests
//! INTENT: Accessibility events, screen reader hooks, global actions
//! IDL (target): setEnabled(bool), subscribeEvents(mask), injectGesture(seq), setFocus(nodeId)
//! DEPS: systemui/compositor (focus), ime (text)
//! READINESS: print "accessibilityd: ready"; register/heartbeat with samgr
//! TESTS: event roundtrip
#[test]
fn hint_output() {
    assert!(accessibilityd::execute(&["zoom"]).contains("zoom"));
}
