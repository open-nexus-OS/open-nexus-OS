// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Accessibility daemon CLI tests
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - Accessibility hint output via CLI
//!
//! TEST_SCENARIOS:
//!   - hint_output(): Verify accessibility hint contains expected keyword
//!
//! DEPENDENCIES:
//!   - accessibilityd::execute: CLI execution function
//!
//! ADR: docs/adr/0017-service-architecture.md
#[test]
fn hint_output() {
    assert!(accessibilityd::execute(&["zoom"]).contains("zoom"));
}
