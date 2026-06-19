// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use nexus_svg::{render_svg, render_svg_tinted, render_svg_tinted_at, SvgError};

#[test]
fn renders_at_hidpi_scale() {
    // Render-at-scale (the asset-pipeline entry): the same icon at 256px covers
    // far more pixels than at 24px and fills the larger canvas — crisp at HiDPI.
    let svg = include_str!("../../../../resources/icons/lucide/icons/wallet.svg");
    let small = render_svg_tinted_at(svg, (255, 255, 255), 24, 24).unwrap();
    let big = render_svg_tinted_at(svg, (255, 255, 255), 256, 256).unwrap();
    assert_eq!((big.width, big.height), (256, 256));
    let count = |o: &nexus_svg::RasterOutput| o.buffer.chunks_exact(4).filter(|p| p[3] > 0).count();
    let (s, b) = (count(&small), count(&big));
    assert!(b > s * 10, "HiDPI render covers far more pixels ({b} vs {s})");
}

#[test]
fn renders_real_lucide_icon() {
    // The real Lucide wallet icon: root stroke="currentColor", two child <path>s
    // (with arcs) that inherit it, round caps/joins. End-to-end proof.
    let svg = include_str!("../../../../resources/icons/lucide/icons/wallet.svg");
    let out = render_svg_tinted(svg, (255, 255, 255)).unwrap();
    let opaque = out.buffer.chunks_exact(4).filter(|p| p[3] > 0).count();
    assert!(opaque > 50, "real Lucide icon renders a visible stroke ({opaque} px)");
}

#[test]
fn inherited_stroke_and_currentcolor_tint() {
    // Lucide pattern: stroke/width/linecap on the root <svg>, fill=none; the child
    // <path> declares none of them and must inherit — and currentColor resolves to
    // the caller's tint. Without the cascade the child would render nothing.
    let svg = r##"<svg width="24" height="24" xmlns="http://www.w3.org/2000/svg"
        fill="none" stroke="currentColor" stroke-width="2"
        stroke-linecap="round" stroke-linejoin="round">
        <path d="M4,12 L20,12" />
    </svg>"##;
    let out = render_svg(svg).unwrap();
    let w = out.width as usize;
    let alpha = |x: usize, y: usize| out.buffer[(y * w + x) * 4 + 3];
    assert!(alpha(12, 12) > 0, "child path inherits the root stroke");

    // currentColor → red tint colors the inherited stroke (BGRA: r at +2, b at +0).
    let red = render_svg_tinted(svg, (255, 0, 0)).unwrap();
    let i = (12 * w + 12) * 4;
    assert!(red.buffer[i + 2] > 200 && red.buffer[i] < 60, "currentColor tinted red");
}

#[test]
fn test_render_simple_rect() {
    let svg = r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <rect x="10" y="20" width="30" height="40" fill="#ff0000" />
    </svg>"##;
    let output = render_svg(svg).unwrap();
    assert_eq!(output.width, 100);
    assert_eq!(output.height, 100);
}

#[test]
fn round_join_fills_outer_corner_of_stroke() {
    // A right-angle stroke (8px) with a round join. The outer corner pixel
    // (22,3) lies outside both segment quads but inside the join disc at the
    // vertex (20,5) — so it must be opaque. Before joins existed this was a gap.
    let svg = r##"<svg width="30" height="30" xmlns="http://www.w3.org/2000/svg">
        <path d="M5,5 L20,5 L20,20" fill="none" stroke="#000000" stroke-width="8" stroke-linejoin="round" />
    </svg>"##;
    let out = render_svg(svg).unwrap();
    let w = out.width as usize;
    let alpha = |x: usize, y: usize| out.buffer[(y * w + x) * 4 + 3];
    assert!(alpha(22, 3) > 0, "round join must fill the outer corner gap");
    // A point well outside the stroke stays empty.
    assert_eq!(alpha(2, 27), 0, "background stays transparent");
}

#[test]
fn test_render_circle() {
    let svg = r##"<svg width="64" height="64" xmlns="http://www.w3.org/2000/svg">
        <circle cx="32" cy="32" r="20" fill="#00ff00" />
    </svg>"##;
    let output = render_svg(svg).unwrap();
    assert_eq!(output.width, 64);
    assert_eq!(output.height, 64);
}

#[test]
fn test_render_simple_path() {
    let svg = r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <path d="M 10,10 L 90,10 L 90,90 L 10,90 Z" fill="#0000ff" />
    </svg>"##;
    let output = render_svg(svg).unwrap();
    assert_eq!(output.width, 100);
}

#[test]
fn overlapping_filled_paths_render_in_document_order() {
    let svg = r##"<svg width="24" height="24" xmlns="http://www.w3.org/2000/svg">
        <path d="M 2,2 L 22,2 L 22,22 L 2,22 Z" fill="#000000" />
        <path d="M 7,7 L 17,7 L 17,17 L 7,17 Z" fill="#ffffff" />
    </svg>"##;
    let output = render_svg(svg).unwrap();
    let center = ((12 * output.width + 12) * 4) as usize;
    assert_eq!(
        &output.buffer[center..center + 4],
        &[0xff, 0xff, 0xff, 0xff],
        "later filled path must paint over earlier filled path"
    );
    let outline = ((4 * output.width + 4) * 4) as usize;
    assert_eq!(
        &output.buffer[outline..outline + 4],
        &[0x00, 0x00, 0x00, 0xff],
        "earlier filled path must remain visible outside the later fill"
    );
}

#[test]
fn test_render_line() {
    let svg = r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <line x1="10" y1="10" x2="90" y2="90" stroke="#000000" stroke-width="2" />
    </svg>"##;
    let output = render_svg(svg).unwrap();
    assert_eq!(output.width, 100);
}

#[test]
fn test_render_with_group_and_transform() {
    let svg = r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <g transform="translate(10,20)">
            <rect x="0" y="0" width="30" height="30" fill="#ff0000" />
        </g>
    </svg>"##;
    let output = render_svg(svg).unwrap();
    assert_eq!(output.width, 100);
}

#[test]
fn test_render_ellipse() {
    let svg = r##"<svg width="100" height="60" xmlns="http://www.w3.org/2000/svg">
        <ellipse cx="50" cy="30" rx="40" ry="20" fill="#888888" />
    </svg>"##;
    let output = render_svg(svg).unwrap();
    assert_eq!(output.width, 100);
}

#[test]
fn test_render_polygon() {
    let svg = r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <polygon points="50,10 90,90 10,90" fill="#ff8800" />
    </svg>"##;
    let output = render_svg(svg).unwrap();
    assert_eq!(output.width, 100);
}

#[test]
fn test_render_rounded_rect() {
    let svg = r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <rect x="10" y="10" width="80" height="80" rx="10" ry="10" fill="#cccccc" />
    </svg>"##;
    let output = render_svg(svg).unwrap();
    assert_eq!(output.width, 100);
}

#[test]
fn test_deterministic_output() {
    let svg = r##"<svg width="50" height="50" xmlns="http://www.w3.org/2000/svg">
        <rect x="10" y="10" width="30" height="30" fill="#ff0000" />
    </svg>"##;
    let r1 = render_svg(svg).unwrap();
    let r2 = render_svg(svg).unwrap();
    assert_eq!(r1.width, r2.width);
    assert_eq!(r1.height, r2.height);
    assert_eq!(r1.buffer, r2.buffer);
}

#[test]
fn test_reject_svg_script_tag() {
    let svg = r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <script>alert(1)</script>
        <rect x="0" y="0" width="10" height="10" fill="#000000" />
    </svg>"##;
    let err = render_svg(svg).unwrap_err();
    assert!(matches!(err, SvgError::UnsupportedElement { .. }));
}

#[test]
fn test_reject_svg_filter_element() {
    let svg = r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <filter id="blur" />
        <rect x="0" y="0" width="10" height="10" fill="#000000" />
    </svg>"##;
    let err = render_svg(svg).unwrap_err();
    assert!(matches!(err, SvgError::UnsupportedElement { .. }));
}

#[test]
fn test_reject_svg_external_reference() {
    let svg = r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <rect x="0" y="0" width="10" height="10" fill="url(http://evil.com/malware.svg)" />
    </svg>"##;
    let err = render_svg(svg).unwrap_err();
    assert!(matches!(err, SvgError::ExternalReference { .. }));
}

#[test]
fn test_reject_dimension_too_large() {
    let svg = r##"<svg width="3000" height="3000" xmlns="http://www.w3.org/2000/svg">
        <rect x="0" y="0" width="10" height="10" fill="#000000" />
    </svg>"##;
    let err = render_svg(svg).unwrap_err();
    assert!(matches!(err, SvgError::DimensionTooLarge { .. }));
}

#[test]
fn test_no_fill_is_transparent() {
    let svg = r##"<svg width="50" height="50" xmlns="http://www.w3.org/2000/svg">
        <rect x="10" y="10" width="30" height="30" fill="none" />
    </svg>"##;
    let output = render_svg(svg).unwrap();
    assert_eq!(output.width, 50);
}

#[test]
fn test_stroke_on_rect() {
    let svg = r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <rect x="10" y="10" width="80" height="80" fill="none" stroke="#000000" stroke-width="2" />
    </svg>"##;
    let output = render_svg(svg).unwrap();
    assert_eq!(output.width, 100);
}

// ---------------------------------------------------------------------------
// Gradients (A5): real per-pixel linear + radial fills
// ---------------------------------------------------------------------------

/// BGRA pixel accessor for a raster output.
fn px(out: &nexus_svg::RasterOutput, x: usize, y: usize) -> (u8, u8, u8, u8) {
    let w = out.width as usize;
    let i = (y * w + x) * 4;
    (out.buffer[i + 2], out.buffer[i + 1], out.buffer[i], out.buffer[i + 3]) // (r,g,b,a)
}

#[test]
fn linear_gradient_objectbbox_is_a_horizontal_ramp() {
    // Default units (objectBoundingBox): red→blue left to right across the rect.
    let svg = r##"<svg width="100" height="40" xmlns="http://www.w3.org/2000/svg">
        <defs>
            <linearGradient id="g" x1="0" y1="0" x2="1" y2="0">
                <stop offset="0%" stop-color="#ff0000" />
                <stop offset="100%" stop-color="#0000ff" />
            </linearGradient>
        </defs>
        <rect x="0" y="0" width="100" height="40" fill="url(#g)" />
    </svg>"##;
    let out = render_svg(svg).unwrap();
    let left = px(&out, 2, 20);
    let right = px(&out, 97, 20);
    let mid = px(&out, 50, 20);
    assert!(left.0 > 200 && left.2 < 60, "left edge red, got {left:?}");
    assert!(right.2 > 200 && right.0 < 60, "right edge blue, got {right:?}");
    // Midpoint is a genuine blend of the two stops, not a flat single colour.
    assert!(mid.0 > 60 && mid.0 < 200 && mid.2 > 60 && mid.2 < 200, "midpoint blended, got {mid:?}");
}

#[test]
fn linear_gradient_userspaceonuse_axis() {
    // userSpaceOnUse: a vertical (top→bottom) black→white axis in user coords.
    let svg = r##"<svg width="40" height="100" xmlns="http://www.w3.org/2000/svg">
        <defs>
            <linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="0" y2="100">
                <stop offset="0" stop-color="#000000" />
                <stop offset="1" stop-color="#ffffff" />
            </linearGradient>
        </defs>
        <rect x="0" y="0" width="40" height="100" fill="url(#g)" />
    </svg>"##;
    let out = render_svg(svg).unwrap();
    let top = px(&out, 20, 2);
    let bottom = px(&out, 20, 97);
    assert!(top.0 < 40, "top near black, got {top:?}");
    assert!(bottom.0 > 215, "bottom near white, got {bottom:?}");
    assert!(bottom.0 > top.0 + 100, "monotonic dark→light down the axis");
}

#[test]
fn radial_gradient_center_to_edge() {
    // Radial: white centre fading to black at the rim (objectBoundingBox default).
    let svg = r##"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg">
        <defs>
            <radialGradient id="g" cx="0.5" cy="0.5" r="0.5">
                <stop offset="0%" stop-color="#ffffff" />
                <stop offset="100%" stop-color="#000000" />
            </radialGradient>
        </defs>
        <rect x="0" y="0" width="100" height="100" fill="url(#g)" />
    </svg>"##;
    let out = render_svg(svg).unwrap();
    let center = px(&out, 50, 50);
    let corner = px(&out, 4, 4);
    let edge = px(&out, 50, 96);
    assert!(center.0 > 230, "centre near white, got {center:?}");
    assert!(edge.0 < 30, "vertical rim near black, got {edge:?}");
    // The corner is outside the r=0.5 circle → clamps to the last (black) stop.
    assert!(corner.0 < 30, "corner clamps to last stop, got {corner:?}");
    assert!(center.0 > edge.0 + 150, "bright centre, dark rim");
}

#[test]
fn gradient_stop_opacity_is_honored() {
    // A fully transparent first stop must leave the background untouched there.
    let svg = r##"<svg width="100" height="20" xmlns="http://www.w3.org/2000/svg">
        <defs>
            <linearGradient id="g" x1="0" y1="0" x2="1" y2="0">
                <stop offset="0%" stop-color="#ffffff" stop-opacity="0" />
                <stop offset="100%" stop-color="#ffffff" stop-opacity="1" />
            </linearGradient>
        </defs>
        <rect x="0" y="0" width="100" height="20" fill="url(#g)" />
    </svg>"##;
    let out = render_svg(svg).unwrap();
    let left = px(&out, 1, 10);
    let right = px(&out, 98, 10);
    assert!(left.3 < 30, "left stop transparent, got alpha {}", left.3);
    assert!(right.3 > 225, "right stop opaque, got alpha {}", right.3);
}

#[test]
fn missing_gradient_ref_renders_nothing() {
    // A fill referencing an undefined gradient resolves to no paint (transparent),
    // never a panic or a stray flat colour.
    let svg = r##"<svg width="40" height="40" xmlns="http://www.w3.org/2000/svg">
        <rect x="0" y="0" width="40" height="40" fill="url(#nope)" />
    </svg>"##;
    let out = render_svg(svg).unwrap();
    let opaque = out.buffer.chunks_exact(4).filter(|p| p[3] > 0).count();
    assert_eq!(opaque, 0, "unresolved gradient ref paints nothing");
}
