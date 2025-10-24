//! CONTEXT: Location daemon CLI tests
//! INTENT: GNSS/network positioning, geofencing, mock
//! IDL (target): getLast(), subscribe(request), setMock(enabled,loc)
//! DEPS: policyd (privacy), time-syncd (time)
//! READINESS: print "locationd: ready"; register/heartbeat with samgr
//! TESTS: getLast mock; subscribe emits
#[test]
fn fix_estimated() {
    assert!(locationd::execute(&[]).contains("fix"));
}
