//! CONTEXT: Clock synchronization system for external time sources
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 unit test
//!
//! PUBLIC API:
//!   - help() -> &'static str: CLI usage string
//!   - execute(args: &[&str]) -> String: CLI execution
//!   - run(): Daemon entry point
//!
//! DEPENDENCIES:
//!   - std::env::args: CLI argument processing
//!
//! ADR: docs/adr/0012-time-sync-architecture.md

#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

/// Returns the CLI usage string.
pub fn help() -> &'static str {
    "time-sync aligns clocks. Usage: time-sync [--help] offset"
}

/// Executes the CLI logic and returns a descriptive message.
pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        return help().to_string();
    }
    if let Some(offset) = args.first() {
        if let Ok(delta) = offset.parse::<i64>() {
            return format!("time sync applying offset {delta} ppm");
        }
    }
    "time-sync awaiting offset".to_string()
}

/// Entry point used by the daemon to process CLI invocations.
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::execute;

    #[test]
    fn parses_offset() {
        let msg = execute(&["-12"]);
        assert!(msg.contains("-12"));
    }
}
