//! CONTEXT: Settings daemon entrypoint wiring to service logic
//! INTENT: System settings (KV, scopes), observer pattern
//! IDL (target): get(ns,key), set(ns,key,val), subscribe(ns,prefix)
//! DEPS: policyd (access)
//! READINESS: print "settingsd: ready"; register/heartbeat with samgr
//! TESTS: get/set roundtrip; subscribe emits
//! Settings daemon entry point: delegates to the userspace library.

fn main() {
    settings::run();
    println!("settingsd: ready");
}
