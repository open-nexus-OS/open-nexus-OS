//! CONTEXT: Log daemon entrypoint wiring to service logic
//! INTENT: Kernel/user logs, ring buffer, filter/subscribe
//! IDL (target): write(tag,level,msg), subscribe(filter), dump()
//! DEPS: policyd (access control)
//! READINESS: print "logd: ready"; register/heartbeat with samgr
//! TESTS: write/dump roundtrip; subscribe emits
fn main() {
    logd::run();
}
