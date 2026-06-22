// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host/std backend for abilitymgr — CLI parity + host-testable helpers.
//!
//! The lifecycle state machine and wire dispatch live in [`crate::lifecycle`] and
//! [`crate::wire`] (shared with the OS-lite loop). This module keeps the thin CLI
//! surface (`help`/`execute`/`run`) and a host `ReadyNotifier`/error type.

use std::string::String;

/// Errors from the abilitymgr host surface.
#[derive(Debug, thiserror::Error)]
pub enum AbilitymgrError {
    /// IPC error (host mock).
    #[error("ipc: {0}")]
    Ipc(String),
}

/// Result type for abilitymgr operations.
pub type AbilitymgrResult<T> = Result<T, AbilitymgrError>;

/// Notifies init once the service reports readiness (host mock).
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from the provided closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Signals readiness to the caller.
    pub fn notify(self) {
        (self.0)();
    }
}

/// CLI help text.
pub fn help() -> &'static str {
    "abilitymgr manages ability lifecycle. Usage: abilitymgr [--help]"
}

/// CLI entry: prints help or a readiness line.
pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        help().to_string()
    } else {
        "ability manager ready".to_string()
    }
}

/// CLI runner used by the host `main`.
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{execute, help};

    #[test]
    fn help_message() {
        assert!(help().contains("abilitymgr"));
    }

    #[test]
    fn execute_help() {
        let output = execute(&["--help"]);
        assert!(output.contains("Usage"));
    }

    #[test]
    fn execute_default_ready() {
        assert!(execute(&[]).contains("ready"));
    }
}
