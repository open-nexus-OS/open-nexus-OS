// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Thermal manager daemon CLI tests
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - Thermal manager status reporting via CLI
//!
//! TEST_SCENARIOS:
//!   - stable_state(): Verify thermal manager reports stable state
//!
//! DEPENDENCIES:
//!   - thermalmgr::execute: CLI execution function
//!
//! ADR: docs/adr/0017-service-architecture.md
#[test]
fn stable_state() {
    assert!(thermalmgr::execute(&[]).contains("stable"));
}
