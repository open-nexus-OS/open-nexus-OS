// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration coverage for the TASK-0054 renderer package contract.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 2 renderer contract tests
//! TEST_SCOPE: Public renderer crate API for frame allocation and fixture-font loading.
//! TEST_SCENARIOS: 64-byte stride/exact buffer length; repo-owned hello-world glyph loading.
//! DEPENDENCIES: `ui_renderer`
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use ui_renderer::{FixtureFont, Frame, RenderResult};

#[test]
fn frame_stride_is_64_byte_aligned_and_buffer_exact() -> RenderResult<()> {
    let frame = Frame::new_checked(3, 2)?;
    assert_eq!(frame.stride().get(), 64);
    assert_eq!(frame.buffer().len(), 128);
    Ok(())
}

#[test]
fn fixture_font_loads_repo_owned_hello_world_glyphs() -> RenderResult<()> {
    let font = FixtureFont::load_default()?;
    for ch in "hello world".chars() {
        assert!(font.glyph(ch).is_some());
    }
    assert_eq!(font.width(), 5);
    assert_eq!(font.height(), 7);
    Ok(())
}
