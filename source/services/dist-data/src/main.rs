//! CONTEXT: Distributed data service entrypoint wiring to service logic
//! INTENT: Distributed KV data (DDS-like), conflict resolution, sync
//! IDL (target): put(ns,key,val), get(ns,key), watch(ns,prefix), sync(peer)
//! DEPS: dsoftbusd (transport), policyd (access control)
//! READINESS: print "dist-data: ready"; register/heartbeat with samgr
//! TESTS: put/get loopback; watch emits change
fn main() {
    dist_data::run();
}
