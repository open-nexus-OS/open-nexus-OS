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
    assert!(systemui::wallpaper_source_is_jpeg());
    assert_eq!(systemui::wallpaper_decoded_size(), (1280, 800));
    // Golden updated when the wallpaper downscale moved from nearest-neighbour to
    // a box (area-average) filter — crisper background, deterministic output.
    assert_eq!(systemui::checksum(), 4_234_514_043);
}
