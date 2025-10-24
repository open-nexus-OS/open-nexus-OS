//! CONTEXT: Resource manager domain logic shared with the daemon
//! INTENT: Allocate and manage localized assets and resources
//! IDL (target): help(), execute(args), run()
//! DEPS: CLI argument parsing
//! READINESS: Host backend ready; OS backend needs resource allocation
//! TESTS: CLI argument handling, help message generation
//! Resource manager domain logic shared with the daemon.

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
