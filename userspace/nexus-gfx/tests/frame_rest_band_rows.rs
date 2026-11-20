// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! C-r against pixels (TASK-0351 P19): the rows a rest band writes are the blur of what lies
//! below the glass's rest rectangle now, byte for byte. A small model: every layer below the
//! pane is opaque and paints its own numbers, the desktop's base paints its own plus the edits
//! made to it; the rows hold a box blur of `BLUR` pixels (the planner's reach, 66, covers it);
//! a write blurs again where its step says (all, or the band grown by the reach); a read keeps
//! them. Whenever the pane is not held its rows must equal a fresh blur — after every write that
//! ends a hold above all. `frame_reach_oracle.rs` judges random scenes; this one judges the
//! hold's band itself (a band cut short or missing the held plans' moves goes red here).

#[path = "frame_support.rs"]
mod support;

use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::{Area, CacheStep, Frame, FrameLayer, Planned};
use support::*;

const BASE: u32 = 0xFFFF_0000;
const W: usize = 800;
const H: usize = 600;
/// The model's blur: a box this many pixels each way (within the planner's reach, 66).
const BLUR: usize = 60;
const REACH: u32 = 66;
const PANE: (u32, u32, u32, u32) = (300, 300, 400, 200);

fn base() -> Command {
    with(layer(0, 0, 1920, 1080), |l| (l.opaque, l.key) = (true, BASE))
}

/// A window of transform id `id` at `x`, `y`.
fn window(id: u32, x: u32, y: u32, w: u32, h: u32) -> Command {
    with(layer(x, y, w, h), |l| (l.layer_id, l.key, l.opaque) = (id, id << 16, true))
}

fn pane() -> Command {
    with(glass(layer(PANE.0, PANE.1, PANE.2, PANE.3), 40), |l| (l.layer_id, l.key) = (2, 0x2_0000))
}

fn no_damage() -> Command {
    Command::Damage { rect: TileRect::NONE, origin: 0 }
}

fn transform(layer_id: u32, dx: i32, opacity: u32) -> Command {
    Command::LayerTransform {
        layer_id,
        dx,
        dy: 0,
        opacity,
        scale: 10_000,
        origin_x: 0,
        origin_y: 0,
    }
}

fn hash(a: u32, b: u32, c: u32) -> u32 {
    let h = a.wrapping_mul(0x9E37_79B9) ^ b.wrapping_mul(0x85EB_CA6B) ^ c.wrapping_mul(0xC2B2_AE35);
    (h ^ (h >> 15)) & 0xFFFF
}

/// The planner, the base's edits, and the pane's rows (its rest rectangle, row by row).
struct Model {
    frame: Frame,
    edits: Vec<(u32, u32, u32, u32)>,
    rows: Vec<u64>,
    /// Writes that ended a hold as a band smaller than the glass, so far.
    rest_bands: u16,
}

impl Model {
    fn new() -> Self {
        let n = (PANE.2 * PANE.3) as usize;
        Self { frame: ready_frame(4096), edits: Vec::new(), rows: vec![0; n], rest_bands: 0 }
    }

    /// What lies below the pane as the plan draws it: the base, then every layer before it.
    fn backdrop(&self, at: usize) -> Vec<u64> {
        let mut img = vec![0u64; W * H];
        for (i, px) in img.iter_mut().enumerate() {
            let (x, y) = ((i % W) as u32, (i / W) as u32);
            let edited = self
                .edits
                .iter()
                .filter(|e| x >= e.0 && x < e.0 + e.2 && y >= e.1 && y < e.1 + e.3);
            *px = u64::from(hash(BASE, x, y)) + 7 * edited.count() as u64;
        }
        let layers = (0..at).filter_map(|i| self.frame.step(i)).map(|s| s.layer);
        for l in layers.filter(|l: &FrameLayer| l.key != BASE) {
            for y in l.dst_y..(l.dst_y + l.height).min(H as u32) {
                for x in l.dst_x..(l.dst_x + l.width).min(W as u32) {
                    img[y as usize * W + x as usize] =
                        u64::from(hash(l.key, x - l.dst_x, y - l.dst_y));
                }
            }
        }
        img
    }

    /// The box blur of `img` at every pixel of the pane inside `region` (`x0, y0, x1, y1`).
    fn blur(img: &[u64], rows: &mut [u64], (x0, y0, x1, y1): (u32, u32, u32, u32)) {
        let mut sums = vec![0u64; (W + 1) * (H + 1)];
        for y in 0..H {
            for x in 0..W {
                sums[(y + 1) * (W + 1) + x + 1] =
                    img[y * W + x] + sums[y * (W + 1) + x + 1] + sums[(y + 1) * (W + 1) + x]
                        - sums[y * (W + 1) + x];
            }
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let (x, y) = (x as usize, y as usize);
                let (a, b) = (x.saturating_sub(BLUR), (x + BLUR + 1).min(W));
                let (c, d) = (y.saturating_sub(BLUR), (y + BLUR + 1).min(H));
                let sum = sums[d * (W + 1) + b] + sums[c * (W + 1) + a]
                    - sums[c * (W + 1) + b]
                    - sums[d * (W + 1) + a];
                let at = (y - PANE.1 as usize) * PANE.2 as usize + (x - PANE.0 as usize);
                rows[at] = sum;
            }
        }
    }

    /// One present, the pane's rows written as its step says; not held, they must be exact.
    fn present(&mut self, cmds: &[Command]) -> Planned {
        self.frame.layers.take_frame(cmds);
        let p = self.frame.plan(DISPLAY, &every_blur);
        let at = (0..p.layers).find(|&i| self.frame.step(i).is_some_and(|s| s.glass.is_some()));
        let at = at.expect("the pane");
        let glass = self.frame.step(at).and_then(|s| s.glass).expect("its glass");
        let img = self.backdrop(at);
        let rect = Area::rect(PANE.0, PANE.1, PANE.2, PANE.3);
        let all = (PANE.0, PANE.1, PANE.0 + PANE.2, PANE.1 + PANE.3);
        match (glass.cache, glass.band.grown(REACH).intersect(rect)) {
            (CacheStep::Write(_), Area::Rect { x0, y0, x1, y1 }) => {
                Self::blur(&img, &mut self.rows, (x0, y0, x1, y1));
            }
            (CacheStep::Write(_), _) => Self::blur(&img, &mut self.rows, all),
            _ => {}
        }
        self.rest_bands += p.by.misses.band;
        if p.holds.held == 0 {
            let mut fresh = vec![0; self.rows.len()];
            Self::blur(&img, &mut fresh, all);
            let off = self.rows.iter().zip(&fresh).position(|(a, b)| a != b);
            let off = off.map(|i| (PANE.0 + i as u32 % PANE.2, PANE.1 + i as u32 / PANE.2));
            assert_eq!(off, None, "rows that miss a change (band {:?})", glass.band);
        }
        p
    }

    /// A change of the base at `r`, named by the present.
    fn edit(&mut self, r: (u32, u32, u32, u32)) -> Command {
        self.edits.push(r);
        damage(r.0, r.1, r.2, r.3)
    }
}

#[test]
fn the_rows_a_rest_band_writes_are_a_full_blur_of_what_lies_below() {
    let mut m = Model::new();
    m.present(&[base(), window(1, 250, 320, 180, 100), pane()]);
    // The window slides 30 px under the held pane.
    for dx in (3..=30).step_by(3) {
        let p = m.present(&[transform(1, dx, 255), no_damage()]);
        assert_eq!(p.holds.held, 1, "held at {dx}");
    }
    // It rests where it went; the base changes elsewhere below the pane in the same plan.
    let changed = m.edit((600, 450, 20, 10));
    let p =
        m.present(&[transform(1, 0, 255), base(), window(1, 280, 320, 180, 100), pane(), changed]);
    assert_eq!((p.writes.backdrop_band, m.rest_bands), (1, 1), "written as a band");
    let quiet = m.present(&[base(), window(1, 280, 320, 180, 100), pane(), no_damage()]);
    assert_eq!((quiet.writes.total(), cache_of(&m.frame, 2)), (0, Some(CacheStep::Read(0))));
}

#[test]
fn the_rows_stay_exact_through_holds_that_end_on_the_mover_s_travel() {
    // The window travels 120 px, 4 a plan: the pane's hold ends every blur radius of travel and
    // each write blurs where the window went meanwhile; at rest the base changed too.
    let mut m = Model::new();
    m.present(&[base(), window(1, 240, 380, 160, 90), pane()]);
    for dx in (4..=120).step_by(4) {
        m.present(&[transform(1, dx, 255), no_damage()]);
    }
    let changed = m.edit((320, 310, 30, 12));
    m.present(&[transform(1, 0, 255), base(), window(1, 360, 380, 160, 90), pane(), changed]);
    assert!(m.rest_bands >= 2, "{} rest bands", m.rest_bands);
}

#[test]
fn the_rows_stay_exact_when_a_layer_comes_below_a_held_glass() {
    // The pane's own window fades; meanwhile a window opens below it.
    let mut m = Model::new();
    m.present(&[base(), window(1, 250, 320, 180, 100), pane()]);
    m.present(&[transform(2, 0, 128), no_damage()]);
    let opened = [
        transform(2, 0, 160),
        base(),
        window(1, 250, 320, 180, 100),
        window(3, 560, 440, 100, 50),
        pane(),
        no_damage(),
    ];
    assert_eq!(m.present(&opened).holds.held, 1);
    m.present(&[transform(2, 0, 200), no_damage()]);
    let changed = m.edit((640, 470, 16, 16));
    m.present(&[transform(2, 0, 255), changed]);
    assert_eq!(m.rest_bands, 1);
}
