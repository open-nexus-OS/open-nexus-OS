//! CONTEXT: Integration tests for service manager CLI functionality
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - CLI command execution
//!   - Service management operations
//!   - Error handling and validation
//!   - Default ready state
//!
//! TEST_SCENARIOS:
//!   - test_default_ready(): Test default ready state
//!
//! DEPENDENCIES:
//!   - samgr::execute: CLI execution function
//!   - In-memory service registry
//!
//! ADR: docs/adr/0004-idl-runtime-architecture.md

#[test]
fn default_ready() {
    let result = samgr::execute(&[]);
    assert!(result.contains("ready"));
}
