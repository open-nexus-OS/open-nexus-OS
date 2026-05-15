// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use nexus_svg::{render_svg, SvgError};

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
