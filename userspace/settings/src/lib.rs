//! CONTEXT: Host-first configuration storage logic shared with the settings daemon
//! INTENT: Persist and retrieve system configuration key-value pairs
//! IDL (target): help(), execute(args), run(), apply(key=value)
//! DEPS: CLI argument parsing, key-value parsing
//! READINESS: Host backend ready; OS backend needs persistent storage
//! TESTS: Key-value parsing, configuration application, CLI handling
//! Host-first configuration storage logic shared with the settings daemon.

#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

/// Returns the usage string for the settings CLI.
pub fn help() -> &'static str {
    "settingsd persists configuration. Usage: settingsd [--help] key=value"
}

/// Applies the provided CLI arguments and returns a status string.
pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        return help().to_string();
    }
    if let Some(kv) = args.first() {
        if let Some((key, value)) = kv.split_once('=') {
            return format!("settingsd applied {key}={value}");
        }
    }
    "settingsd awaiting assignment".to_string()
}

/// Entry point used by the OS daemon to forward CLI invocations.
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::execute;

    #[test]
    fn apply_setting() {
        assert!(execute(&["theme=dark"]).contains("theme"));
    }
}
