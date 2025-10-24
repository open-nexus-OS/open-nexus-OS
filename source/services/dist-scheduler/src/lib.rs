//! CONTEXT: Distributed scheduler daemon domain library (service API and handlers)
//! INTENT: Remote ability start/continuation, device listing
//! IDL (target): startRemoteAbility(device,intent), continueAbility(token), listDevices()
//! DEPS: dsoftbusd, abilitymgr, samgrd
//! READINESS: print "dist-scheduler: ready"; register/heartbeat with samgr
//! TESTS: listDevices empty; startRemoteAbility mock
pub fn help() -> &'static str {
    "dist-scheduler coordinates remote tasks. Usage: dist-scheduler [--help] delay_ms"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        return help().to_string();
    }
    if let Some(delay) = args.first() {
        if let Ok(ms) = delay.parse::<u64>() {
            let deadline = nexus_sched::Deadline::from_ms(ms);
            return format!("distributed job scheduled at {} ticks", deadline.ticks);
        }
    }
    "dist-scheduler awaiting delay".to_string()
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
    fn computes_deadline() {
        let msg = execute(&["5"]);
        assert!(msg.contains("5000"));
    }
}
