// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Battery manager daemon CLI tests
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - Battery status reporting via CLI
//!
//! TEST_SCENARIOS:
//!   - nominal_status(): Verify battery manager reports nominal status
//!
//! DEPENDENCIES:
//!   - batterymgr::execute: CLI execution function
//!
//! ADR: docs/adr/0017-service-architecture.md
#[test]
fn nominal_status() {
    assert!(batterymgr::execute(&[]).contains("nominal"));
}
