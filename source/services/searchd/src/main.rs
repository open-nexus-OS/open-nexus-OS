//! CONTEXT: Search daemon entrypoint wiring to service logic
//! INTENT: System-wide search/index, suggest
//! IDL (target): index(doc), query(q,opts), suggest(prefix)
//! DEPS: packagefsd/vfsd (data sources)
//! READINESS: print "searchd: ready"; register/heartbeat with samgr
//! TESTS: index/query mock; suggest empty
//! Search daemon entry point.

fn main() {
    search::run();
    println!("searchd: ready");
}
