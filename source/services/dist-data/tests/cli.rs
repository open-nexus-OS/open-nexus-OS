//! CONTEXT: Distributed data service CLI tests
//! INTENT: Distributed KV data (DDS-like), conflict resolution, sync
//! IDL (target): put(ns,key,val), get(ns,key), watch(ns,prefix), sync(peer)
//! DEPS: dsoftbusd (transport), policyd (access control)
//! READINESS: print "dist-data: ready"; register/heartbeat with samgr
//! TESTS: put/get loopback; watch emits change
#[test]
fn sync_message_contains_bus() {
    assert!(dist_data::execute(&["node8"]).contains("dsoftbus"));
}
