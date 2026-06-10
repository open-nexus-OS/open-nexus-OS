// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Distributed data service CLI tests
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - Distributed data sync via dsoftbus transport
//!
//! TEST_SCENARIOS:
//!   - sync_message_contains_bus(): Verify sync message references dsoftbus
//!
//! DEPENDENCIES:
//!   - dist_data::execute: CLI execution function
//!
//! ADR: docs/adr/0017-service-architecture.md
#[test]
fn sync_message_contains_bus() {
    assert!(dist_data::execute(&["node8"]).contains("dsoftbus"));
}
