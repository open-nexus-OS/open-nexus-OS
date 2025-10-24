//! CONTEXT: Clipboard daemon entrypoint wiring to service logic
//! INTENT: System clipboard management (text/binary), change notifications
//! IDL (target): set(data,mime), get(), subscribe()
//! DEPS: systemui/ime (UX integration), policyd (optional access policy)
//! READINESS: print "clipboardd: ready"; register/heartbeat with samgr
//! TESTS: loopback set/get roundtrip; subscribe emits change event
//! Clipboard daemon entry point.

fn main() {
    clipboard::run();
    println!("clipboardd: ready");
}
