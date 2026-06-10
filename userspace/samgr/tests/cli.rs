// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for service manager CLI functionality
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - CLI command execution and service management operations
//!
//! TEST_SCENARIOS:
//!   - default_ready(): Test default ready state
//!
//! DEPENDENCIES:
//!   - samgr::execute: CLI execution function
//!
//! ADR: docs/adr/0017-service-architecture.md

#[test]
fn default_ready() {
    let result = samgr::execute(&[]);
    assert!(result.contains("ready"));
}
