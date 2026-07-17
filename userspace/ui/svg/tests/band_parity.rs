// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Band-parity proof for the compute-broker SVG job (SMP track Phase D4):
//! rasterizing a plan in disjoint row bands — any split, any order — must be
//! byte-identical to one full rasterize. This is the `workers=1 ≡ workers=N`
//! equality contract at the library level.

use nexus_svg::{parse_svg, plan_document_at, rasterize_document_at, OUTPUT_BYTES_PER_PIXEL};

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64">
  <rect x="4" y="4" width="56" height="56" fill="#284a6e"/>
  <circle cx="24" cy="28" r="16" fill="#e0a33c" fill-opacity="0.8"/>
  <circle cx="40" cy="36" r="18" fill="#5ac8fa" fill-opacity="0.6"/>
</svg>"##;

#[test]
fn bands_reproduce_full_rasterize_for_any_split() {
    let doc = parse_svg(SVG).expect("parse");
    let (w, h) = (64u32, 64u32);
    let full = rasterize_document_at(&doc, w, h).expect("full").buffer;
    assert_eq!(full.len(), (w * h) as usize * OUTPUT_BYTES_PER_PIXEL);

    let plan = plan_document_at(&doc, w, h).expect("plan");
    for bands in [1u32, 2, 3, 4, 5, h] {
        let mut assembled = vec![0u8; full.len()];
        // Fresh scratch per "worker" — parity must not depend on reuse.
        for b in 0..bands {
            let y0 = h * b / bands;
            let y1 = h * (b + 1) / bands;
            let mut scratch = plan.scratch();
            let out =
                &mut assembled[(y0 * w) as usize * OUTPUT_BYTES_PER_PIXEL
                    ..(y1 * w) as usize * OUTPUT_BYTES_PER_PIXEL];
            plan.rasterize_rows(y0, y1, &mut scratch, out).expect("band");
        }
        assert_eq!(assembled, full, "band split x{bands} must be byte-identical");
    }

    // Shared-scratch reuse across bands must also be identical.
    let mut scratch = plan.scratch();
    let mut assembled = vec![0u8; full.len()];
    for b in 0..4u32 {
        let y0 = h * b / 4;
        let y1 = h * (b + 1) / 4;
        let out = &mut assembled[(y0 * w) as usize * OUTPUT_BYTES_PER_PIXEL
            ..(y1 * w) as usize * OUTPUT_BYTES_PER_PIXEL];
        plan.rasterize_rows(y0, y1, &mut scratch, out).expect("band");
    }
    assert_eq!(assembled, full, "scratch reuse must not change bytes");
}
