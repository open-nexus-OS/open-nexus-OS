// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! What a held glass owes for a change made while the layers below it are displaced, against
//! pixels (TASK-0351 P19-e). The model of `frame_rest_band_rows.rs` — opaque layers painting
//! their own numbers, a box blur of `BLUR` pixels, a write blurring where its step says — with
//! the layers' content changes painted too. Whenever the pane is neither held nor hiding a write
//! its rows must equal a fresh blur: a change owed where it does not rest, or a displacement not
//! paid, goes red here. The rule: displaced, a change is owed within the reach with no cover
//! asked, together with what the layers below changed since the rows' key; a write put off on a
//! key miss owes the band C-c explains; a lost change list owes the whole glass (`hold.rs`).

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

/// An opaque window of transform id `id` (key `id << 16`) at `x`, `y`.
fn window(id: u32, x: u32, y: u32, w: u32, h: u32) -> Command {
    with(layer(x, y, w, h), |l| (l.layer_id, l.key, l.opaque) = (id, id << 16, true))
}

/// The glass of window 2, or of the desktop (id 0) for `desk`.
fn pane(desk: bool) -> Command {
    let id = u32::from(!desk) * 2;
    with(glass(layer(PANE.0, PANE.1, PANE.2, PANE.3), 40), |l| (l.layer_id, l.key) = (id, 0x9_0000))
}

fn no_damage() -> Command {
    Command::Damage { rect: TileRect::NONE, origin: 0 }
}

fn transform(layer_id: u32, dx: i32) -> Command {
    Command::LayerTransform {
        layer_id,
        dx,
        dy: 0,
        opacity: 255,
        scale: 10_000,
        origin_x: 0,
        origin_y: 0,
    }
}

fn hash(a: u32, b: u32, c: u32) -> u32 {
    let h = a.wrapping_mul(0x9E37_79B9) ^ b.wrapping_mul(0x85EB_CA6B) ^ c.wrapping_mul(0xC2B2_AE35);
    (h ^ (h >> 15)) & 0xFFFF
}

/// The planner, the content changes made so far — `(key, x, y, w, h)`, in the layer's own
/// pixels (the base's are the screen's) — and the pane's rows (its rest rectangle).
struct Model {
    frame: Frame,
    edits: Vec<(u32, u32, u32, u32, u32)>,
    rows: Vec<u64>,
}

impl Model {
    fn new() -> Self {
        let n = (PANE.2 * PANE.3) as usize;
        Self { frame: ready_frame(4096), edits: Vec::new(), rows: vec![0; n] }
    }

    /// Layer `key`'s pixel at its own `x`, `y`: its numbers and its changes.
    fn paint(&self, key: u32, x: u32, y: u32) -> u64 {
        let edited = self
            .edits
            .iter()
            .filter(|e| e.0 == key && x >= e.1 && x < e.1 + e.3 && y >= e.2 && y < e.2 + e.4);
        u64::from(hash(key, x, y)) + 7 * edited.count() as u64
    }

    /// What lies below the pane as the plan draws it: the base, then every layer before it.
    fn backdrop(&self, at: usize) -> Vec<u64> {
        let mut img = vec![0u64; W * H];
        for (i, px) in img.iter_mut().enumerate() {
            *px = self.paint(BASE, (i % W) as u32, (i / W) as u32);
        }
        let layers = (0..at).filter_map(|i| self.frame.step(i)).map(|s| s.layer);
        for l in layers.filter(|l: &FrameLayer| l.key != BASE) {
            for y in l.dst_y..(l.dst_y + l.height).min(H as u32) {
                for x in l.dst_x..(l.dst_x + l.width).min(W as u32) {
                    img[y as usize * W + x as usize] = self.paint(l.key, x - l.dst_x, y - l.dst_y);
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

    /// The pane's step index in the last plan.
    fn pane_at(&self, p: &Planned) -> usize {
        let at = (0..p.layers).find(|&i| self.frame.step(i).is_some_and(|s| s.glass.is_some()));
        at.expect("the pane")
    }

    /// One present, the pane's rows written as its step says; neither held nor hiding a write,
    /// they must be exact.
    fn present(&mut self, cmds: &[Command]) -> Planned {
        self.frame.layers.take_frame(cmds);
        let p = self.frame.plan(DISPLAY, &every_blur);
        let at = self.pane_at(&p);
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
        if p.holds.held == 0 && p.writes.hidden == 0 {
            let mut fresh = vec![0; self.rows.len()];
            Self::blur(&img, &mut fresh, all);
            let off = self.rows.iter().zip(&fresh).position(|(a, b)| a != b);
            let off = off.map(|i| (PANE.0 + i as u32 % PANE.2, PANE.1 + i as u32 / PANE.2));
            assert_eq!(off, None, "rows that miss a change (band {:?})", glass.band);
        }
        p
    }

    /// A change of layer `key` at its own `x, y, w, h`, named by the present where the layer is
    /// drawn now (`at`: its drawn corner).
    fn edit(&mut self, key: u32, r: (u32, u32, u32, u32), at: (u32, u32)) -> Command {
        self.edits.push((key, r.0, r.1, r.2, r.3));
        let (x, y) = if key == BASE { (r.0, r.1) } else { (at.0 + r.0, at.1 + r.1) };
        Command::Damage { rect: TileRect { x, y, width: r.2, height: r.3 }, origin: key }
    }

    /// The pane's glass step in the last plan.
    fn glass(&self, p: &Planned) -> nexus_gfx::frame::Glass {
        self.frame.step(self.pane_at(p)).and_then(|s| s.glass).expect("its glass")
    }
}

#[test]
fn test_reject_a_rest_band_missing_a_change_covered_during_the_hold() {
    // A static window (3) lies below the pane; a cover (1) beside its corner slides over it
    // while the pane is held, the window changes under the cover, the cover slides back. The
    // change rests where it was named: owed though a cover stood over it then.
    let mut m = Model::new();
    let scene = |m: &mut Model| {
        m.present(&[
            base(),
            window(3, 320, 320, 200, 100),
            window(1, 340, 380, 60, 60),
            pane(false),
        ])
    };
    scene(&mut m);
    for dx in (1..=5).map(|k| -6 * k) {
        assert_eq!(m.present(&[transform(1, dx), no_damage()]).holds.held, 1, "held at {dx}");
    }
    // The cover stands at 310..370 × 380..440: the window's change at 330..350 × 390..400 is
    // under it.
    let changed = m.edit(3 << 16, (10, 70, 20, 10), (320, 320));
    let p = m.present(&[transform(1, -30), changed]);
    assert_eq!((p.holds.held, p.holds.foreign), (1, 1), "held, owing the covered change");
    // Back where the rows were blurred (a key hit): the write pays the change.
    let p = m.present(&[transform(1, 0), no_damage()]);
    assert_eq!((m.glass(&p).cache, p.by.rest), (CacheStep::Write(0), 1));
    assert!(m.glass(&p).band != Area::Full, "a band, not the whole glass");
}

#[test]
fn a_moving_layers_change_is_paid_by_the_displacement() {
    // The window (1) slides 32 px right under the held pane; mid-way it changes in a part that
    // is out of the pane's reach then (x < 234) and inside it at rest — no owe names it, the
    // displacement pays where the window rests.
    let mut m = Model::new();
    m.present(&[base(), window(1, 170, 380, 100, 90), pane(false)]);
    for dx in (4..=32).step_by(4) {
        let named = if dx == 16 {
            m.edit(1 << 16, (30, 10, 14, 10), (170 + dx as u32, 380))
        } else {
            no_damage()
        };
        let p = m.present(&[transform(1, dx), named]);
        assert_eq!((p.holds.held, p.holds.foreign), (1, 0), "held at {dx}, nothing owed");
    }
    let rest = [transform(1, 0), base(), window(1, 202, 380, 100, 90), pane(false), no_damage()];
    let p = m.present(&rest);
    assert_eq!((p.by.rest, p.writes.backdrop_band, p.by.misses.band), (1, 1, 1), "a band");
}

#[test]
fn test_reject_a_bounded_owe_missing_a_mover_s_change_back_where_it_was() {
    // The window (1) moves under the held pane, changes where it is drawn displaced, and comes
    // back where the rows were blurred (a key hit): the change rests 30 px left of where it was
    // named — the owe carries the displacement so far, so the write reaches it.
    let mut m = Model::new();
    m.present(&[base(), window(1, 280, 360, 150, 100), pane(false)]);
    m.present(&[transform(1, 30), no_damage()]);
    let changed = m.edit(1 << 16, (60, 20, 20, 10), (310, 360));
    let p = m.present(&[transform(1, 30), changed]);
    assert_eq!((p.holds.held, p.holds.foreign), (1, 1));
    let p = m.present(&[transform(1, 0), no_damage()]);
    assert_eq!((m.glass(&p).cache, p.by.rest), (CacheStep::Write(0), 1));
    assert!(m.glass(&p).band != Area::Full, "a band, not the whole glass");
}

#[test]
fn test_reject_a_put_off_owe_smaller_than_its_explanation() {
    // An opaque window (4) hides the pane while the window below it (1) moves twice and the
    // base changes: each key miss is put off owing what C-c explains (never the whole glass),
    // the second on top of the first's owe. Uncovered, the write pays it all.
    let mut m = Model::new();
    let cover = window(4, 280, 280, 440, 240);
    let scene = |x: u32| [base(), window(1, x, 380, 100, 90), pane(false), cover.clone()];
    let mut first = scene(200).to_vec();
    first.push(no_damage());
    m.present(&first);
    let p = m.present(&[scene(230).as_slice(), &[no_damage()]].concat());
    assert_eq!((p.writes.hidden, p.writes.total()), (1, 0), "put off");
    let changed = m.edit(BASE, (600, 450, 20, 10), (0, 0));
    let p = m.present(&[scene(260).as_slice(), &[changed]].concat());
    assert_eq!((p.writes.hidden, p.writes.total()), (1, 0), "put off again");
    let owed = m.frame.glass.owed(2);
    assert!(matches!(owed, Area::Rect { .. }), "a band: {owed:?}");
    let p = m.present(&[base(), window(1, 260, 380, 100, 90), pane(false), no_damage()]);
    assert_eq!((m.glass(&p).cache, m.glass(&p).band), (CacheStep::Write(0), owed));
}

#[test]
fn test_reject_a_bounded_owe_after_a_lost_change_list() {
    // A change list that found no room cannot be reached through a present (its room is the
    // present's bound plus every step); the rule is pinned where it lives: `hold.rs`
    // (`test_reject_a_bounded_owe_after_a_lost_change_list`) and `reach.rs` (`uncovered` of a
    // lost list is the whole glass). Through a plan: a full present ends every hold and writes
    // the whole glass.
    let mut m = Model::new();
    m.present(&[base(), window(1, 280, 360, 150, 100), pane(false)]);
    assert_eq!(m.present(&[transform(1, 10), no_damage()]).holds.held, 1);
    let p = m.present(&[transform(1, 20), base(), window(1, 280, 360, 150, 100), pane(false)]);
    assert_eq!((p.holds.held, m.glass(&p).band), (0, Area::Full));
}

#[test]
fn a_desktop_glass_owes_no_change_inside_its_own_region() {
    // A desktop glass (id 0) held over a window that moves below it: a change of the desktop
    // inside its own region is the region's content (R1), uncovered too — no owe; back at rest
    // the rows are read. One beside the region, within its reach, is owed.
    let mut f = ready_frame(4096);
    let scene = [base(), window(1, 280, 360, 150, 100), pane(true)];
    let present = |f: &mut Frame, cmds: &[Command]| {
        f.layers.take_frame(cmds);
        f.plan(DISPLAY, &every_blur)
    };
    present(&mut f, &scene);
    present(&mut f, &[transform(1, 20), no_damage()]);
    let p = present(&mut f, &[transform(1, 20), damage(500, 400, 20, 10)]);
    assert_eq!((p.holds.held, p.holds.foreign), (1, 0), "its own content");
    let p = present(&mut f, &[transform(1, 0), no_damage()]);
    assert_eq!((p.writes.total(), cache_of(&f, 2)), (0, Some(CacheStep::Read(0))));
    present(&mut f, &[transform(1, 20), no_damage()]);
    let p = present(&mut f, &[transform(1, 20), damage(250, 400, 20, 10)]);
    assert_eq!((p.holds.held, p.holds.foreign), (1, 1), "beside it");
    present(&mut f, &[transform(1, 0), no_damage()]);
    let g = f.step(2).and_then(|s| s.glass).expect("the glass");
    assert_eq!(g.cache, CacheStep::Write(0));
    assert!(Area::rect(250, 400, 20, 10).intersect(g.band) != Area::None, "{:?}", g.band);
}
