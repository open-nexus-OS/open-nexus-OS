// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Compositor daemon headless tests
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - Compositor frame checksum validation
//!
//! TEST_SCENARIOS:
//!   - composed_checksum(): Verify compositor checksum matches expected value
//!
//! DEPENDENCIES:
//!   - compositor::checksum: Frame checksum computation
//!
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md
#[test]
fn composed_checksum() {
    assert_eq!(compositor::checksum(), 15_196_384);
}
