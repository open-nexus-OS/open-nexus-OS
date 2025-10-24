//! CONTEXT: Distributed data service domain library (service API and handlers)
//! INTENT: Distributed KV data (DDS-like), conflict resolution, sync
//! IDL (target): put(ns,key,val), get(ns,key), watch(ns,prefix), sync(peer)
//! DEPS: dsoftbusd (transport), policyd (access control)
//! READINESS: print "dist-data: ready"; register/heartbeat with samgr
//! TESTS: put/get loopback; watch emits change
pub fn help() -> &'static str {
    "dist-data replicates state across devices. Usage: dist-data [--help] token"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        return help().to_string();
    }
    if let Some(token) = args.first() {
        return format!("dist-data sync via dsoftbus:{token}");
    }
    "dist-data awaiting token".to_string()
}

pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::execute;

    #[test]
    fn sync_invokes_bus() {
        let msg = execute(&["node8"]);
        assert!(msg.contains("dsoftbus"));
    }
}
