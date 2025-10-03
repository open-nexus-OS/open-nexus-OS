//! Resource manager domain logic shared with the daemon.

#![forbid(unsafe_code)]

#[cfg(all(feature = "backend-host", feature = "backend-os"))]
compile_error!("Choose exactly one backend feature.");

#[cfg(not(any(feature = "backend-host", feature = "backend-os")))]
compile_error!("Select a backend feature.");

/// Returns the CLI usage string.
pub fn help() -> &'static str {
    "resourcemgr allocates localized assets. Usage: resourcemgr [--help]"
}

/// Executes the CLI command and returns its message.
pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
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
