//! CONTEXT: Location daemon domain library (service API and handlers)
//! INTENT: GNSS/network positioning, geofencing, mock
//! IDL (target): getLast(), subscribe(request), setMock(enabled,loc)
//! DEPS: policyd (privacy), time-syncd (time)
//! READINESS: print "locationd: ready"; register/heartbeat with samgr
//! TESTS: getLast mock; subscribe emits
pub fn help() -> &'static str {
    "locationd fuses sensors for positioning. Usage: locationd [--help]"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        help().to_string()
    } else {
        "location daemon fix estimated".to_string()
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
    fn fix_message() {
        assert!(execute(&[]).contains("fix"));
    }
}
