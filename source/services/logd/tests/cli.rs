//! CONTEXT: Log daemon CLI tests
//! INTENT: Kernel/user logs, ring buffer, filter/subscribe
//! IDL (target): write(tag,level,msg), subscribe(filter), dump()
//! DEPS: policyd (access control)
//! READINESS: print "logd: ready"; register/heartbeat with samgr
//! TESTS: write/dump roundtrip; subscribe emits
#[test]
fn capture_log() {
    assert!(logd::execute(&["event"]).contains("event"));
}
