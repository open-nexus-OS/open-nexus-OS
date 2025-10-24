//! CONTEXT: Accessibility daemon entrypoint wiring to service logic
//! INTENT: Accessibility events, screen reader hooks, global actions
//! IDL (target): setEnabled(bool), subscribeEvents(mask), injectGesture(seq)
//! DEPS: systemui/compositor (focus), ime (text)
//! READINESS: print "accessibilityd: ready"; register/heartbeat with samgr
//! TESTS: event subscribe/inject gesture roundtrip
fn main() {
    accessibilityd::run();
}
