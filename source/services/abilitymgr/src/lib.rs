//! CONTEXT: Ability Manager daemon domain library (service API and handlers)
//! INTENT: Ability/feature lifecycle (start/stop/connect/terminate), focus/foreground mgmt, continuation
//! IDL (target): startAbility(intent), stopAbility(id), connectAbility(id), terminateAbility(id), queryAbilities(filter)
//! DEPS: samgr (resolve), bundlemgrd (manifest/required caps), dsoftbusd (continuation)
//! READINESS: print "abilitymgr: ready"; register/heartbeat with samgr
//! TESTS: start/stop loopback OK
pub fn help() -> &'static str {
    "abilitymgr manages ability lifecycle. Usage: abilitymgr [--help]"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        help().to_string()
    } else {
        "ability manager ready".to_string()
    }
}

pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{execute, help};

    #[test]
    fn help_message() {
        assert!(help().contains("abilitymgr"));
    }

    #[test]
    fn execute_help() {
        let output = execute(&["--help"]);
        assert!(output.contains("Usage"));
    }
}
