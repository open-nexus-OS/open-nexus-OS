//! CONTEXT: Time sync daemon entrypoint wiring to service logic
//! INTENT: NTP/PTP sync, clock management
//! IDL (target): syncNow(), setServer(url), subscribe()
//! DEPS: identityd (optional TLS), policyd (net policy)
//! READINESS: print "time-syncd: ready"; register/heartbeat with samgr
//! TESTS: syncNow mock; subscribe emits
//! Time synchronization daemon entry point.

fn main() {
    time_sync::run();
    println!("time-syncd: ready");
}
