//! CONTEXT: Thermal manager daemon domain library (service API and handlers)
//! INTENT: Thermal sensing, throttling/hints
//! IDL (target): subscribe(sensor), setThrottling(level), getTemp(sensor)
//! DEPS: powermgr (policy coordination)
//! READINESS: print "thermalmgr: ready"; register/heartbeat with samgr
//! TESTS: getTemp mock; subscribe emits
pub fn help() -> &'static str {
    "thermalmgr balances device thermals. Usage: thermalmgr [--help]"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        help().to_string()
    } else {
        "thermal manager stable".to_string()
    }
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
    fn stable_message() {
        assert!(execute(&[]).contains("stable"));
    }
}
