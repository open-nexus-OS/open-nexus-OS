//! Identity validation logic shared with the OS daemon.

#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

/// Returns the CLI usage string for the identity service.
pub fn help() -> &'static str {
    "identity validates distributed principals. Usage: identity [--help]"
}

/// Validates whether `token` is an acceptable identity credential.
pub fn validate(token: &str) -> bool {
    token.len() >= 4 && token.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Executes the CLI command.
pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        help().to_string()
    } else if let Some(token) = args.first() {
        if validate(token) {
            format!("identity accepted {token}")
        } else {
            "identity rejected token".to_string()
        }
    } else {
        "identity idle".to_string()
    }
}

/// Entry point used by the daemon to forward CLI requests.
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{execute, validate};

    #[test]
    fn rejects_short_token() {
        assert!(!validate("ab"));
    }

    #[test]
    fn accepts_valid_token() {
        assert!(validate("node1"));
        assert!(execute(&["node1"]).contains("accepted"));
    }
}
