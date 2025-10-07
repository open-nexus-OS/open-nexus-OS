//! User notification dispatch logic shared with the OS daemon.

#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

/// Returns the CLI usage string for notificationd.
pub fn help() -> &'static str {
    "notificationd brokers user alerts. Usage: notificationd [--help]"
}

/// Executes the CLI request, returning a human-readable status.
pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        help().to_string()
    } else {
        "notification dispatcher listening".to_string()
    }
}

/// Entry point used by the OS daemon to forward CLI arguments.
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::execute;

    #[test]
    fn dispatcher_message() {
        assert!(execute(&[]).contains("dispatcher"));
    }
}
