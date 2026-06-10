// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Input Method Engine daemon CLI tests
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - IME text transform via CLI
//!
//! TEST_SCENARIOS:
//!   - uppercase_cli(): Verify IME transforms input to uppercase
//!
//! DEPENDENCIES:
//!   - ime::execute: CLI execution function
//!
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md
#[test]
fn uppercase_cli() {
    assert_eq!(ime::execute(&["xyz"]), "XYZ");
}
