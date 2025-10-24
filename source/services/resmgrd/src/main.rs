//! CONTEXT: Resource manager daemon entrypoint wiring to service logic
//! INTENT: QoS/quota, CPU set/memory/IO limits
//! IDL (target): setQos(task,class), setLimit(ns,val), getUsage(task)
//! DEPS: execd (PIDs), policyd (policy checks)
//! READINESS: print "resmgrd: ready"; register/heartbeat with samgr
//! TESTS: setQos mock; getUsage returns 0
//! Resource manager daemon entry point.

fn main() {
    resmgr::run();
    println!("resmgrd: ready");
}
