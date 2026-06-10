// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Location daemon CLI tests
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - Location fix estimation via CLI
//!
//! TEST_SCENARIOS:
//!   - fix_estimated(): Verify location daemon reports fix estimated
//!
//! DEPENDENCIES:
//!   - locationd::execute: CLI execution function
//!
//! ADR: docs/adr/0017-service-architecture.md
#[test]
fn fix_estimated() {
    assert!(locationd::execute(&[]).contains("fix"));
}
