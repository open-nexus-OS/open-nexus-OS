// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for DSoftBus CLI functionality
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - Service discovery status
//!   - CLI argument parsing
//!   - Node health checking
//!
//! TEST_SCENARIOS:
//!   - test_healthy_status(): Test node health status reporting
//!
//! DEPENDENCIES:
//!   - dsoftbus::execute: CLI execution function
//!   - Service discovery backend
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[test]
fn healthy_status() {
    assert!(dsoftbus::execute(&["--status", "node9"]).contains("healthy"));
}
