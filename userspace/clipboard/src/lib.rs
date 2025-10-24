// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Clipboard storage and management
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 unit test, 1 integration test
//!
//! PUBLIC API:
//!   - help() -> &'static str: CLI usage string
//!   - execute(args: &[&str]) -> String: CLI execution
//!   - run(): Daemon entry point
//!
//! DEPENDENCIES:
//!   - std::sync::Mutex: Thread-safe storage
//!   - std::env::args: CLI argument processing
//!
//! ADR: docs/adr/0008-clipboard-architecture.md

#![forbid(unsafe_code)]

use std::sync::Mutex;

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

static CLIPBOARD: Mutex<Option<String>> = Mutex::new(None);

/// Returns the CLI usage string for clipboardd.
pub fn help() -> &'static str {
    "clipboard stores shared text. Usage: clipboard [--help] [--set value]"
}

/// Executes the clipboard CLI and returns a response string.
pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        return help().to_string();
    }
    if let Some(pos) = args.iter().position(|arg| *arg == "--set") {
        if let Some(value) = args.get(pos + 1) {
            if let Ok(mut guard) = CLIPBOARD.lock() {
                *guard = Some((*value).to_string());
            }
            return format!("clipboard updated to {value}");
        }
    }
    CLIPBOARD
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or_else(|| "clipboard empty".to_string())
}

/// Entry point used by the daemon to forward CLI arguments.
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{execute, CLIPBOARD};

    #[test]
    fn set_and_get() {
        let _ = CLIPBOARD.lock().map(|mut guard| *guard = None);
        execute(&["--set", "hello"]);
        assert!(execute(&[]).contains("hello"));
    }
}
