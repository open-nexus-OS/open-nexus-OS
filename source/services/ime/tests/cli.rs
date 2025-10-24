//! CONTEXT: Input Method Engine daemon CLI tests
//! INTENT: Input method (keyboard), compose/commit, candidate UI
//! IDL (target): attachClient(win), setComposingText(text), commitText(text), show/hide()
//! DEPS: systemui, compositor, accessibilityd
//! READINESS: print "ime: ready"; register/heartbeat with samgr
//! TESTS: attachClient mock; transform uppercase
#[test]
fn uppercase_cli() {
    assert_eq!(ime::execute(&["xyz"]), "XYZ");
}
