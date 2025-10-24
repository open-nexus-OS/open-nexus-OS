//! CONTEXT: Location daemon entrypoint wiring to service logic
//! INTENT: GNSS/network location, geofencing, mock location
//! IDL (target): getLast(), subscribe(request), setMock(enabled,loc)
//! DEPS: policyd (privacy), time-syncd (clock)
//! READINESS: print "locationd: ready"; register/heartbeat with samgr
//! TESTS: subscribe mock location
fn main() {
    locationd::run();
}
