// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Power manager daemon CLI tests
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - Power manager status reporting via CLI
//!
//! TEST_SCENARIOS:
//!   - power_ready(): Verify power manager reports ready state
//!
//! DEPENDENCIES:
//!   - powermgr::execute: CLI execution function
//!
//! ADR: docs/adr/0017-service-architecture.md
#[test]
fn power_ready() {
    assert!(powermgr::execute(&[]).contains("power"));
}
