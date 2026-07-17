// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Anti-aliasing proof for the CPU SVG rasterizer. A 1-sample/pixel
//! fill produces only 0/255 alpha at edges (hard, jagged). The coverage-based
//! rasterizer must produce intermediate alphas along curved/diagonal edges.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_SCOPE: nexus_svg coverage anti-aliasing

use nexus_svg::render_svg;

/// A filled circle must have a ring of partially-covered (anti-aliased) edge
/// pixels — alpha strictly between 0 and 255.
#[test]
fn circle_edge_is_antialiased() {
    let out = render_svg(
        r##"<svg width="64" height="64"><circle cx="32" cy="32" r="24" fill="#ffffff" /></svg>"##,
    )
    .unwrap();
    // BGRA → alpha is byte 3 of each pixel.
    let mut partial = 0usize;
    let mut opaque = 0usize;
    for px in out.buffer.chunks_exact(4) {
        match px[3] {
            0 => {}
            255 => opaque += 1,
            _ => partial += 1,
        }
    }
    eprintln!("circle: opaque={opaque} partial(AA)={partial}");
    assert!(opaque > 1000, "interior must be solid");
    // A 64px circle has a perimeter ~150px; with 4x AA expect dozens+ of
    // fractional-coverage edge pixels. A hard-edged fill would give 0.
    assert!(partial > 60, "expected an anti-aliased edge ring, got {partial}");
}

/// A 45° diagonal edge (the classic jaggies case) must produce graduated
/// coverage rather than a hard staircase.
#[test]
fn diagonal_edge_is_antialiased() {
    let out = render_svg(
        r##"<svg width="48" height="48"><path d="M 0 0 L 48 48 L 0 48 Z" fill="#ffffff" /></svg>"##,
    )
    .unwrap();
    let partial = out.buffer.chunks_exact(4).filter(|px| px[3] > 0 && px[3] < 255).count();
    eprintln!("diagonal: partial(AA)={partial}");
    assert!(partial > 20, "diagonal edge must be anti-aliased, got {partial}");
}

/// Dump a circle to a PPM for eyeballing (ignored by default; run with
/// `cargo test -p nexus-svg --test antialias_test dump_circle_ppm -- --ignored --nocapture`).
#[test]
#[ignore]
fn dump_circle_ppm() {
    let out = render_svg(
        r##"<svg width="96" height="96"><circle cx="48" cy="48" r="40" fill="#3b82f6" /></svg>"##,
    )
    .unwrap();
    let mut ppm = format!("P6\n{} {}\n255\n", out.width, out.height).into_bytes();
    // BGRA → RGB over a white background (so AA edges read as light blue).
    for px in out.buffer.chunks_exact(4) {
        let (b, g, r, a) = (px[0] as u32, px[1] as u32, px[2] as u32, px[3] as u32);
        let over = |c: u32| ((c * a + 255 * (255 - a)) / 255) as u8;
        ppm.push(over(r));
        ppm.push(over(g));
        ppm.push(over(b));
    }
    std::fs::write("/tmp/svg_aa_circle.ppm", ppm).unwrap();
    eprintln!("wrote /tmp/svg_aa_circle.ppm ({}x{})", out.width, out.height);
}

/// Elliptical arcs (`a`/`A`) must parse AND render (previously rejected as
/// InvalidPathCommand, then silently skipped in tessellation). A rounded-rect
/// path built from arcs must fill a substantial, anti-aliased area.
#[test]
fn arc_path_parses_and_renders() {
    // A pill/rounded shape: line + arc + line + arc + close.
    let svg = r##"<svg width="80" height="48"><path d="M 24 8 L 56 8 A 16 16 0 0 1 56 40 L 24 40 A 16 16 0 0 1 24 8 Z" fill="#22c55e" /></svg>"##;
    let out = nexus_svg::render_svg(svg).expect("arc path must parse");
    let opaque = out.buffer.chunks_exact(4).filter(|px| px[3] == 255).count();
    let partial = out.buffer.chunks_exact(4).filter(|px| px[3] > 0 && px[3] < 255).count();
    eprintln!("arc pill: opaque={opaque} partial(AA)={partial}");
    assert!(opaque > 1500, "arc-built shape must fill substantial area, got {opaque}");
    assert!(partial > 40, "arc edges must be anti-aliased, got {partial}");
}

/// A semicircle drawn with a single `A` command must round (curved edge),
/// not collapse to a chord (which the old silent-skip produced).
#[test]
fn arc_is_curved_not_chord() {
    let svg = r##"<svg width="64" height="64"><path d="M 8 32 A 24 24 0 0 1 56 32 Z" fill="#ffffff" /></svg>"##;
    let out = nexus_svg::render_svg(svg).unwrap();
    let w = out.width as usize;
    // Top-centre pixel (x=32, y=10) is inside the arc dome but ABOVE the chord
    // line (y=32) — only a real curve covers it.
    let idx = (10 * w + 32) * 4;
    assert!(out.buffer[idx + 3] > 0, "arc dome must cover pixels above the chord");
}

/// Dump the arc pill for eyeballing.
#[test]
#[ignore]
fn dump_arc_ppm() {
    let svg = r##"<svg width="120" height="72"><path d="M 36 12 L 84 12 A 24 24 0 0 1 84 60 L 36 60 A 24 24 0 0 1 36 12 Z" fill="#3b82f6" /></svg>"##;
    let out = nexus_svg::render_svg(svg).unwrap();
    let mut ppm = format!("P6\n{} {}\n255\n", out.width, out.height).into_bytes();
    for px in out.buffer.chunks_exact(4) {
        let (b, g, r, a) = (px[0] as u32, px[1] as u32, px[2] as u32, px[3] as u32);
        let over = |c: u32| ((c * a + 255 * (255 - a)) / 255) as u8;
        ppm.push(over(r));
        ppm.push(over(g));
        ppm.push(over(b));
    }
    std::fs::write("/tmp/svg_arc_pill.ppm", ppm).unwrap();
    eprintln!("wrote /tmp/svg_arc_pill.ppm");
}
