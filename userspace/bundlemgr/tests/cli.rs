// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for bundle manager CLI functionality
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 2 CLI tests
//!
//! TEST_SCOPE:
//!   - Bundle installation flow
//!   - Bundle removal flow
//!   - CLI command execution
//!   - Ability registrar integration
//!
//! TEST_SCENARIOS:
//!   - test_install_flow(): Test bundle installation via CLI
//!   - test_remove_flow(): Test bundle removal via CLI
//!
//! DEPENDENCIES:
//!   - bundlemgr::execute: CLI execution function
//!   - StubRegistrar: Mock ability registrar for testing
//!   - Test bundle files (apps/test-signed.nxb)
//!
//! ADR: docs/adr/0009-bundle-manager-architecture.md

struct StubRegistrar;

impl bundlemgr::AbilityRegistrar for StubRegistrar {
    fn register(&self, ability: &str) -> Result<Vec<u8>, String> {
        Ok(vec![ability.len() as u8])
    }
}

#[test]
fn install_flow() {
    let output = bundlemgr::execute(&["install", "apps/test-signed.nxb"], &StubRegistrar);
    assert!(output.contains("bundle installed"));
}

#[test]
fn remove_flow() {
    let output = bundlemgr::execute(&["remove", "launcher"], &StubRegistrar);
    assert!(output.contains("removed"));
}
