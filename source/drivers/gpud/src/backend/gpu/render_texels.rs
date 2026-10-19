// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A texture named at any texel (TASK-0215 5c-3 P4b). The board's frame names a surface's
//! rectangle by its descriptor's address — an atlas band at any column, a glass's cache rows one
//! texel in — where the rungs before sampled at 128-byte steps alone. The CPU writes a strip
//! whose texels carry their column (blue) and row (green); the opaque layer draws an 8x8
//! rectangle of it named 1, 2, 3, 4 and 8 texels past an aligned column, and two pixels of each
//! read back name the texel the unit fetched: `gpud: gpu texel offsets ok (k=… read=…)`. A read
//! a few texels short is the alignment the unit applied to the address (the FAIL form says it).

use alloc::format;
use alloc::string::String;

use nexus_gfx::backend::gpu::quads::{Bump, Quad, Template};

use super::device_v2::Device;
use super::render_blur::{descriptor, plane};
use super::render_cost::{named, peek, run, Rig, STRIDE};
use super::render_template::COST_TAILS;
use crate::backend::display::{Display, GpuCanvas};

/// The strip: 64x8 texels from an aligned column (4096 bytes into its row), clear of the other
/// rungs' pictures.
const STRIP: (u32, u32) = (1024, 900);
const STRIP_W: u32 = 64;
/// The rectangles' offsets from the strip's column, in texels.
const OFFSETS: [u32; 5] = [1, 2, 3, 4, 8];
/// Each rectangle drawn at `DRAWN` plus 16 columns per offset, `SIDE` square.
const DRAWN: (u32, u32) = (1200, 900);
const SIDE: u32 = 8;
/// The pixels read in each rectangle (its first, and one inside).
const READS: [(u32, u32); 2] = [(0, 0), (5, 3)];
const RED: u8 = 0x80;
const GREEN: u8 = 0x40;

/// The strip written into the plane (memory order B, G, R, A), each row cleaned.
fn write_strip(display: &mut dyn Display, canvas: &GpuCanvas) -> bool {
    let Some(bytes) = display.gpu_canvas_bytes(canvas.row) else { return false };
    for dy in 0..SIDE {
        let at = (STRIP.1 + dy) as usize * STRIDE + STRIP.0 as usize * 4;
        let Some(row) = bytes.get_mut(at..at + 4 * STRIP_W as usize) else { return false };
        for (i, px) in row.chunks_exact_mut(4).enumerate() {
            px.copy_from_slice(&[i as u8, GREEN + dy as u8, RED, 0xff]);
        }
        if canvas.block != 0 {
            nexus_abi::cache_clean(row, canvas.block);
        }
    }
    true
}

/// Each offset's rectangle drawn and read: the line's part, and whether every read fetched the
/// texel its address names.
fn offsets(
    dev: &mut Device,
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    rig: &Rig<'_>,
    t: &Template<'_>,
    place: &mut Bump,
) -> Result<(String, bool), String> {
    if !write_strip(display, canvas) {
        return Err(String::from("no cpu view of the plane"));
    }
    // Every pass before was waited: the cost rung's tails are free again.
    *place = COST_TAILS;
    let opaque = named(dev, rig, "layer-opaque")?;
    let (mut k_list, mut read_list, mut all) = (String::new(), String::new(), true);
    for (i, k) in OFFSETS.into_iter().enumerate() {
        let image = plane(rig, (STRIP.0 + k, STRIP.1), SIDE, SIDE);
        let texture_va = descriptor(dev, rig, place, &image)?;
        let (x, y) = (DRAWN.0 + 16 * i as u32, DRAWN.1);
        let q = Quad::layer(x as f32, y as f32, SIDE as f32, SIDE as f32, [1.0; 4], 0.0);
        run(dev, rig, &Template { texture_va, ..*t }, &opaque, place, &[q], true)?;
        let mut fetched = [0u8; READS.len()];
        for (f, (dx, dy)) in fetched.iter_mut().zip(READS) {
            let px = peek(display, canvas, ((x + dx) as usize, (y + dy) as usize))
                .ok_or("a read outside the plane")?;
            // The column the unit fetched: blue less the read's own column in the rectangle.
            *f = px[0].wrapping_sub(dx as u8);
            all &= px[0] == (k + dx) as u8 && px[1] == GREEN + dy as u8 && px[2] == RED;
        }
        let sep = if i == 0 { "" } else { "/" };
        k_list.push_str(&format!("{sep}{k}"));
        read_list.push_str(&format!("{sep}{}", fetched[0]));
        if fetched[1] != fetched[0] {
            read_list.push_str(&format!(":{}", fetched[1]));
        }
    }
    Ok((format!("(k={k_list} read={read_list})"), all))
}

/// The rung's line: `gpud: gpu texel offsets ok (…)`, or its FAIL form with what was read.
pub(super) fn rung(
    dev: &mut Device,
    display: &mut dyn Display,
    canvas: &GpuCanvas,
    rig: &Rig<'_>,
    t: &Template<'_>,
    place: &mut Bump,
) {
    super::emit(&match offsets(dev, display, canvas, rig, t, place) {
        Ok((line, true)) => format!("gpud: gpu texel offsets ok {line}"),
        Ok((line, false)) => format!("gpud: FAIL gpu texel offsets {line}"),
        Err(e) => format!("gpud: FAIL gpu texel offsets ({e})"),
    });
}
