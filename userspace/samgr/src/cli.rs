//! User-facing CLI helpers shared with the OS daemon.

/// Returns the CLI usage string for the service ability manager.
pub fn help() -> &'static str {
    "samgr orchestrates service abilities. Usage: samgr [--help]"
}

/// Executes the CLI using provided arguments.
pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        help().to_string()
    } else {
        "service ability manager ready".to_string()
    }
}

/// Parses `std::env::args` and prints the execution result.
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{execute, help};

    #[test]
    fn help_contains_name() {
        assert!(help().contains("samgr"));
    }

    #[test]
    fn exec_default() {
        assert!(execute(&[]).contains("ready"));
    }
}
