//! CONTEXT: Input method editor (IME) daemon entrypoint wiring to service logic
//! INTENT: compose/commit text, candidate UI, input focus
//! IDL (target): attachClient(win), setComposingText(text), commitText(text), show(), hide()
//! DEPS: systemui/compositor (UI), accessibilityd (a11y)
//! READINESS: print "ime: ready"; register/heartbeat with samgr
//! TESTS: compose/commit loopback
fn main() {
    ime::run();
}
