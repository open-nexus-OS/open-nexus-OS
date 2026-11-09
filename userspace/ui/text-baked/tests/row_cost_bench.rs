// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0351 P16 — what one text run costs per row in the CPU painter, measured on the
//! host (`cargo test -p nexus-text-baked --release --test row_cost_bench -- --ignored
//! --nocapture`): the glyph blend, and the per-row `ellipsis_cut` app-host runs before it.

use nexus_text_baked::{draw_text_row, ellipsis_cut, line_height, measure, FontSize, Tracking};
use std::time::Instant;

#[test]
#[ignore]
fn row_cost_of_a_text_run() {
    let text = "Notifications and quiet hours";
    let none = Tracking::NONE;
    for (label, size) in [
        ("13/400", FontSize::S13W400),
        ("14/400", FontSize::S14W400),
        ("20/600", FontSize::S20W600),
    ] {
        let lh = line_height(size);
        let w = measure(text.chars(), size, none);
        let mut row = vec![0u8; 1920 * 4];
        let t0 = Instant::now();
        for _ in 0..200 {
            for y in 0..lh {
                draw_text_row(&mut row, y, 0, 600, 1920, text.chars(), size, none, [255; 4]);
            }
        }
        let draw = t0.elapsed().as_nanos() as u64 / (200 * u64::from(lh));
        let t0 = Instant::now();
        for _ in 0..200 {
            for _ in 0..lh {
                std::hint::black_box(ellipsis_cut(text, size, none, w));
            }
        }
        let cut = t0.elapsed().as_nanos() as u64 / (200 * u64::from(lh));
        let t0 = Instant::now();
        for _ in 0..200 {
            for _ in 0..lh {
                std::hint::black_box(ellipsis_cut(text, size, none, w / 2));
            }
        }
        let cut_half = t0.elapsed().as_nanos() as u64 / (200 * u64::from(lh));
        println!(
            "{label:<6} {} chars w={w}px lh={lh}: draw {draw} ns/row, ellipsis_cut fits {cut} \
             ns/row, cut {cut_half} ns/row",
            text.len()
        );
    }
}
