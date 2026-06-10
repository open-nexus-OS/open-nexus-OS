// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service ability manager CLI – command-line interface for samgr operations
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 2 unit tests (help_contains_name, exec_default)
//! ADR: docs/adr/0017-service-architecture.md

/// Returns the CLI usage string for the service ability manager.
pub fn help() -> &'static str {
    "samgr orchestrates service abilities. Usage: samgr [--help]"
}

/// Executes the CLI using provided arguments.
pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
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
