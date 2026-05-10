// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Golden and PNG metadata-independent host snapshot proof.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 snapshot/golden integration test
//! TEST_SCOPE: Canonical BGRA golden comparison and deterministic PNG artifacts.
//! TEST_SCENARIOS: Hex golden compare; PNG roundtrip; gamma/iCCP chunks do not affect pixels.
//! DEPENDENCIES: `ui_host_snap`, `ui_renderer`
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use std::error::Error;
use std::fs;
use std::path::Path;

use ui_host_snap::{
    artifact_root, bgra_to_rgba, compare_hex_golden, decode_png_rgba, encode_png_rgba, golden_root,
    hex_bytes, insert_chunk_after_ihdr, make_damage, temp_artifact_path, GoldenMode,
};
use ui_renderer::{Frame, PixelBgra, Rect};

#[test]
fn snapshot_golden_comparison_is_canonical_and_metadata_independent() -> Result<(), Box<dyn Error>>
{
    let mut frame = Frame::new_checked(4, 4)?;
    let mut damage = make_damage(&frame, 4)?;
    frame.clear(PixelBgra::new(0, 0, 0, 0xff), &mut damage)?;
    frame.draw_rect(Rect::new(1, 1, 2, 2)?, PixelBgra::from_rgba(0xff, 0, 0, 0xff), &mut damage)?;
    let bgra = frame.logical_bgra_bytes()?;
    let actual = hex_bytes(&bgra)?;
    compare_hex_golden(
        &golden_root(),
        Path::new("clear_rect.bgra.hex"),
        &actual,
        GoldenMode::CompareOnly,
    )?;

    let rgba = bgra_to_rgba(4, 4, &bgra)?;
    let png = encode_png_rgba(4, 4, &rgba)?;
    let artifact_path = temp_artifact_path("clear_rect.png")?;
    assert!(artifact_path.starts_with(artifact_root()?));
    fs::write(artifact_path, &png)?;
    let decoded = decode_png_rgba(&png)?;
    assert_eq!(decoded.width, 4);
    assert_eq!(decoded.height, 4);
    assert_eq!(decoded.rgba, rgba);

    let with_gamma = insert_chunk_after_ihdr(&png, *b"gAMA", &[0, 0, 0, 1])?;
    let with_iccp = insert_chunk_after_ihdr(&png, *b"iCCP", b"fixture\0\0")?;
    assert_eq!(decode_png_rgba(&with_gamma)?.rgba, rgba);
    assert_eq!(decode_png_rgba(&with_iccp)?.rgba, rgba);
    Ok(())
}
