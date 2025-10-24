//! CONTEXT: Thermal manager daemon CLI tests
//! INTENT: Thermal sensing, throttling/hints
//! IDL (target): subscribe(sensor), setThrottling(level), getTemp(sensor)
//! DEPS: powermgr (policy coordination)
//! READINESS: print "thermalmgr: ready"; register/heartbeat with samgr
//! TESTS: getTemp mock; subscribe emits
#[test]
fn stable_state() {
    assert!(thermalmgr::execute(&[]).contains("stable"));
}
