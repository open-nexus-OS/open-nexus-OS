// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Search daemon for local content indexing
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test, 1 integration test
//!
//! PUBLIC API:
//!   - help() -> &'static str: CLI usage string
//!   - execute(args: &[&str]) -> String: CLI execution
//!   - run(): Daemon entry point
//!
//! DEPENDENCIES:
//!   - std::env::args: CLI argument processing
//!
//! ADR: docs/adr/0010-search-architecture.md

#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

/// Returns the CLI usage string.
pub fn help() -> &'static str {
    "searchd indexes local content. Usage: searchd [--help]"
}

/// Executes the search CLI command.
pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        help().to_string()
    } else {
        "search daemon indexing".to_string()
    }
}

/// Entry point used by the daemon to process CLI arguments.
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::execute;

    #[test]
    fn indexing_message() {
        assert!(execute(&[]).contains("indexing"));
    }
}
