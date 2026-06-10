// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SVG rendering integration tests covering basic shapes (rect, path, circle, line)
//! and the cursor SVG arrow shape. Validates that render_svg produces non-trivial pixel output.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_SCOPE: SVG renderer basic shape and cursor rendering
//! TEST_SCENARIOS: 1 test (test_all_svg_render: 5 sub-scenarios — rect, path-rect, circle,
//! line, cursor arrow)
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#[test]
fn test_all_svg_render() {
    use nexus_svg::render_svg;

    // 1. Simple rect
    let r1 = render_svg(r##"<svg width="100" height="100"><rect x="10" y="10" width="80" height="80" fill="#ff0000" /></svg>"##).unwrap();
    let nz1 = r1.buffer.iter().filter(|&&b| b != 0).count();
    eprintln!("rect 80x80: non-zero={}/{}", nz1, r1.buffer.len());
    assert!(nz1 > 1000, "rect must render substantial pixels");

    // 2. Path rect
    let r2 = render_svg(r##"<svg width="100" height="100"><path d="M 10 10 L 90 10 L 90 90 L 10 90 Z" fill="#0000ff" /></svg>"##).unwrap();
    let nz2 = r2.buffer.iter().filter(|&&b| b != 0).count();
    eprintln!("path rect: non-zero={}/{}", nz2, r2.buffer.len());
    assert!(nz2 > 1000, "path rect must render substantial pixels");

    // 3. Circle
    let r3 = render_svg(
        r##"<svg width="64" height="64"><circle cx="32" cy="32" r="20" fill="#00ff00" /></svg>"##,
    )
    .unwrap();
    let nz3 = r3.buffer.iter().filter(|&&b| b != 0).count();
    eprintln!("circle: non-zero={}/{}", nz3, r3.buffer.len());
    assert!(nz3 > 500, "circle must render substantial pixels");

    // 4. Line with stroke
    let r4 = render_svg(r##"<svg width="100" height="100"><line x1="10" y1="10" x2="90" y2="90" stroke="#000000" stroke-width="2" /></svg>"##).unwrap();
    let nz4 = r4.buffer.iter().filter(|&&b| b != 0).count();
    eprintln!("line: non-zero={}/{}", nz4, r4.buffer.len());
    assert!(nz4 > 0, "line must render at least some pixels");

    // 5. CURSOR SVG
    let r5 = render_svg(r##"<svg width="32" height="32" xmlns="http://www.w3.org/2000/svg">
    <path d="M 5,5 L 25,18 L 18,20 L 22,28 L 17,30 L 13,22 Z" fill="#ffffff" stroke="#000000" stroke-width="1.5" />
</svg>"##).unwrap();
    let nz5 = r5.buffer.iter().filter(|&&b| b != 0).count();
    eprintln!("CURSOR: non-zero={}/{}", nz5, r5.buffer.len());
    assert!(nz5 > 100, "CURSOR must render substantial pixels (arrow shape)");

    eprintln!("ALL SVG TESTS PASSED!");
}
