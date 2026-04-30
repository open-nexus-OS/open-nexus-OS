// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SystemUI host tests for TOML-backed first-frame composition.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: TOML-backed profile/shell seed and deterministic first frame.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#[test]
fn systemui_checksum() {
    assert_eq!(systemui::checksum(), 1_999_217_024);
}
