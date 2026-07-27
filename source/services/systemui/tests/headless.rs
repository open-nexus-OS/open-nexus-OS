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
    // a box (area-average) filter — crisper background, deterministic output —
    // and again when the bake stopped STRETCHING the source onto the panel and
    // started covering it (centred crop to the target aspect, `object-fit:
    // cover` per the design contract). The source is 3:2 and the panel 8:5, so
    // the old mapping squashed the image ~7%; the new one crops 32 rows off the
    // top and bottom instead. Different pixels, same determinism.
    assert_eq!(systemui::checksum(), 3_519_376_773);
}
