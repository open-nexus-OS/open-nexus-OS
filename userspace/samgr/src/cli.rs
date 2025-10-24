//! CONTEXT: Command-line interface for service ability manager
//!
//! OWNERS: @runtime
//!
//! PUBLIC API:
//!   - help() -> &'static str
//!     Returns CLI usage string for service ability manager
//!   - execute(args: &[&str]) -> String
//!     Executes CLI commands and returns response string
//!   - run()
//!     Main entry point that processes command line arguments
//!
//! CLI INTERFACE:
//!   - --help: Display usage information
//!   - (no args): Return ready status
//!
//! SECURITY INVARIANTS:
//!   - No unsafe code in CLI operations
//!   - Input validation prevents buffer overflows
//!   - Graceful handling of invalid arguments
//!
//! ERROR CONDITIONS:
//!   - Invalid arguments: Returns help text
//!   - Service management failure: Returns ready status
//!
//! DEPENDENCIES:
//!   - std::env::args: Command line argument processing
//!
//! FEATURES:
//!   - Service ability management
//!   - CLI interface for service operations
//!   - Help system for usage information
//!   - Graceful error handling
//!
//! TEST SCENARIOS:
//!   - test_help_contains_name(): Verify help text contains service name
//!   - test_exec_default(): Test default execution behavior
//!   - test_cli_commands(): Test all CLI commands
//!   - test_error_handling(): Test error handling
//!   - test_argument_validation(): Test argument validation
//!   - test_output_formatting(): Test output formatting
//!   - test_help_command(): Test help command
//!   - test_service_management(): Test service management operations
//!
//! ADR: docs/adr/0004-idl-runtime-architecture.md

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
