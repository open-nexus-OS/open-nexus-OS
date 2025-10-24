//! CONTEXT: Thermal manager daemon entrypoint wiring to service logic
//! INTENT: Thermal sensing, throttling/hints
//! IDL (target): subscribe(sensor), setThrottling(level), getTemp(sensor)
//! DEPS: powermgr (policy coordination)
//! READINESS: print "thermalmgr: ready"; register/heartbeat with samgr
//! TESTS: getTemp mock; subscribe emits
fn main() {
    thermalmgr::run();
}
