// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Media service CLI tests
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - Media pipeline probe via CLI
//!
//! TEST_SCENARIOS:
//!   - probe_ready(): Verify media pipeline ready for specified asset
//!
//! DEPENDENCIES:
//!   - media::execute: CLI execution function
//!
//! ADR: docs/adr/0017-service-architecture.md
#[test]
fn probe_ready() {
    assert!(media::execute(&["--probe", "clip.mp4"]).contains("clip.mp4"));
}
