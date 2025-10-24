//! CONTEXT: Ability manager entrypoint wiring to service logic
//! INTENT: Ability/feature lifecycle (start/stop/connect/terminate), focus/foreground mgmt
//! IDL (target): startAbility(intent), stopAbility(id), connectAbility(id), terminateAbility(id)
//! DEPS: samgrd (resolve), bundlemgrd (manifest caps), dsoftbusd (continuation)
//! READINESS: print "abilitymgr: ready"; register/heartbeat with samgr
//! TESTS: start/stop roundtrip; resolve via samgr loopback
fn main() {
    abilitymgr::run();
}
