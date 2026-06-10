// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Distributed scheduler daemon domain library – service API and CLI handlers
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test (computes_deadline)
//! ADR: docs/adr/0017-service-architecture.md
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
