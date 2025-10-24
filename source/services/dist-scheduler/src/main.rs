//! CONTEXT: Distributed scheduler daemon entrypoint wiring to service logic
//! INTENT: Remote ability start/continuation, device listing
//! IDL (target): startRemoteAbility(device,intent), continueAbility(token), listDevices()
//! DEPS: dsoftbusd, abilitymgr, samgrd
//! READINESS: print "dist-scheduler: ready"; register/heartbeat with samgr
//! TESTS: listDevices empty; startRemoteAbility mock
fn main() {
    dist_scheduler::run();
}
