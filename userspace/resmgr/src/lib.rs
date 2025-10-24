//! CONTEXT: Resource manager for localized asset allocation
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
//! ADR: docs/adr/0015-resource-manager-architecture.md

#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

/// Returns the CLI usage string.
pub fn help() -> &'static str {
    "resourcemgr allocates localized assets. Usage: resourcemgr [--help]"
}

/// Executes the CLI command and returns its message.
pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        help().to_string()
    } else {
        "resource manager initialized".to_string()
    }
}

/// Entry point used by the daemon to drive the CLI.
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::execute;

    #[test]
    fn default_message() {
        assert!(execute(&[]).contains("initialized"));
    }
}
