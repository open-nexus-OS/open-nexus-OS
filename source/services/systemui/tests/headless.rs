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
    assert_eq!(systemui::wallpaper_decoded_size(), nexus_display_proto::layout::LAYOUT_MAX);
    // Golden updated when the wallpaper downscale moved from nearest-neighbour to
    // a box (area-average) filter — crisper background, deterministic output —
    // and again when the bake stopped STRETCHING the source onto the panel and
    // started covering it (centred crop to the target aspect, `object-fit:
    // cover` per the design contract). The source is 3:2 and the panel 8:5, so
    // the old mapping squashed the image ~7%; the new one crops 32 rows off the
    // top and bottom instead. Different pixels, same determinism. Updated once more
    // for M-L (2026-09-30): the frame is baked at the layout maximum, 1920x1080 (the
    // 1536x1024 source is covered onto 16:9 — an asset at or above the maximum is a
    // follow-up), so every pixel moved. And for TASK-0251 P2a step 3 (2026-10-04): the bake
    // resamples with Lanczos-3 (the box filter fell back to nearest-neighbour above 1:1, so the
    // 1.25× upscale of the 1536-wide source showed stair steps on every ridge); the checksum
    // was measured twice, identical.
    assert_eq!(systemui::checksum(), 1_717_839_154);
}
