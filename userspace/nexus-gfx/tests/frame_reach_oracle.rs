// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The planner against a full redraw (TASK-0351 P6): 200 random presents — rectangles named with
//! their layer's key, with origin 0, with a key the set does not hold and from above; layers
//! that move, swap their order, open and close; full presents — drawn by a model executor that
//! does what a plan says and nothing else: it blurs a glass into its cache rows where the step
//! writes (the band grown by the blur's reach, or the whole glass), draws it from those rows
//! where the step reads, and redraws the plane inside the plan's damage alone. Two properties:
//!
//! - DAMAGE: the plane, redrawn inside each plan's damage only, is what the executor would draw
//!   everywhere — every pixel a frame changes lies in its damage. Held for any scene.
//! - ROWS: what the executor draws from its rows is what a full redraw draws, byte for byte —
//!   every change reached every glass whose backdrop it is. Held where the planner defers
//!   nothing: no window in motion (blur at rest), no write put off under what hides it, every
//!   glass a window of its own. The first sequence is built so (its glasses are pills, whose
//!   backs hide nothing, over every opaque layer); the second lets square glasses and opaque
//!   layers lie anywhere, so writes are put off, and holds DAMAGE alone.
//!
//! The model: a pixel is a number; an opaque layer replaces it, a translucent one mixes its
//! content over it, a glass mixes its content over the sum of what lies below within `BLUR`
//! pixels (a box — any change in it changes the sum).

use nexus_gfx::command::buffer::{Command, LAYER_OPAQUE};
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::layers::ORIGIN_ABOVE;
use nexus_gfx::frame::{Area, CacheStep, Frame, FrameLayer, Glass, WritesBy};
use std::collections::HashMap;

const W: u32 = 256;
const H: u32 = 192;
const DISPLAY: (u32, u32) = (W, H);
/// The planner's reach of a glass (`frame::reach::GLASS_REACH`): what a band write blurs again.
const REACH: u32 = 66;
/// The model's blur: a box of this radius — nearly the reach, so a change two blurs away from
/// a glass (through a glass below it) is felt, as the widest blur a layer may ask would.
const BLUR: u32 = 60;
/// The blur a glass layer asks for (the planner's rows follow it; the model's box does not).
const ASKED: u32 = 40;
const FRAMES: u32 = 200;
const POINTER: u32 = 8;

type Rect = (u32, u32, u32, u32);

struct Rng(u32);

impl Rng {
    fn below(&mut self, n: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 % n
    }

    fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent
    }
}

fn hash(a: u32, b: u32, c: u32) -> u32 {
    let h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA6B) ^ c.wrapping_mul(0xC2B2_AE35);
    (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D)
}

fn mix(below: u32, content: u32) -> u32 {
    below.rotate_left(7).wrapping_mul(0x9E37_79B1) ^ content
}

fn at(x: u32, y: u32) -> usize {
    (y * W + x) as usize
}

fn wallpaper() -> Vec<u32> {
    (0..W * H).map(|i| hash(7, i % W, i / W)).collect()
}

/// The box sums of `img` around every pixel of `region` (inside `rect`), written into the
/// glass's grid `out` (`rect`'s pixels, row by row).
fn blur_into(img: &[u32], rect: Rect, region: Rect, out: &mut [u32]) {
    let stride = (W + 1) as usize;
    let mut sums = vec![0u32; stride * (H + 1) as usize];
    for y in 0..H as usize {
        let mut row = 0u32;
        for x in 0..W as usize {
            row = row.wrapping_add(img[y * W as usize + x]);
            sums[(y + 1) * stride + x + 1] = sums[y * stride + x + 1].wrapping_add(row);
        }
    }
    let sum = |x: u32, y: u32| sums[y as usize * stride + x as usize];
    for y in region.1..region.1 + region.3 {
        for x in region.0..region.0 + region.2 {
            let (x0, y0) = (x.saturating_sub(BLUR), y.saturating_sub(BLUR));
            let (x1, y1) = ((x + BLUR + 1).min(W), (y + BLUR + 1).min(H));
            let boxed = sum(x1, y1)
                .wrapping_sub(sum(x0, y1))
                .wrapping_sub(sum(x1, y0))
                .wrapping_add(sum(x0, y0));
            out[((y - rect.1) * rect.2 + (x - rect.0)) as usize] = boxed;
        }
    }
}

fn blur_of(img: &[u32], rect: Rect) -> Vec<u32> {
    let mut out = vec![0; (rect.2 * rect.3) as usize];
    blur_into(img, rect, rect, &mut out);
    out
}

/// One layer of the scene: what the present sends, and the model's content of its pixels.
#[derive(Clone)]
struct Item {
    layer: FrameLayer,
    content: Vec<u32>,
}

impl Item {
    fn rect(&self) -> Rect {
        (self.layer.dst_x, self.layer.dst_y, self.layer.width, self.layer.height)
    }

    fn glass(&self) -> bool {
        self.layer.backdrop_blur != 0
    }

    fn command(&self) -> Command {
        let l = &self.layer;
        Command::CompositeLayer {
            src_row_abs: l.src_row_abs,
            src_x: l.src_x,
            width: l.width,
            height: l.height,
            dst_x: l.dst_x,
            dst_y: l.dst_y,
            opacity: l.opacity,
            corner_radius: l.corner_radius,
            shadow_blur: l.shadow_blur,
            shadow_offset_y: l.shadow_offset_y,
            shadow_alpha: l.shadow_alpha,
            backdrop_blur: l.backdrop_blur,
            content_w: l.content_w,
            content_h: l.content_h,
            scroll_band_top_abs: l.scroll_band_top_abs,
            scroll_band_h: l.scroll_band_h,
            layer_id: l.layer_id,
            content_epoch: l.content_epoch,
            flags: if l.opaque { LAYER_OPAQUE } else { 0 },
            key: l.key,
            source: l.source,
        }
    }

    /// The layer drawn over `img`: a glass over `back` (its blurred backdrop, its own grid).
    fn paint(&self, img: &mut [u32], back: Option<&[u32]>) {
        let (x0, y0, w, h) = self.rect();
        for ly in 0..h {
            for lx in 0..w {
                let (i, p) = ((ly * w + lx) as usize, at(x0 + lx, y0 + ly));
                img[p] = match back {
                    Some(back) => mix(back[i], self.content[i]),
                    None if self.layer.opaque => self.content[i],
                    None => mix(img[p], self.content[i]),
                };
            }
        }
    }
}

struct World {
    items: Vec<Item>,
    pointer: (u32, u32),
    serial: u32,
    /// Glasses are square and layers lie in any order (writes are put off); else glasses are
    /// pills and no opaque layer lies above one.
    square: bool,
}

impl World {
    fn new(rng: &mut Rng, square: bool) -> Self {
        let layer = FrameLayer {
            src_row_abs: 1000,
            width: W,
            height: H,
            opacity: 255,
            opaque: true,
            key: 0xFFFF_0000,
            ..FrameLayer::EMPTY
        };
        let base = Item { layer, content: (0..W * H).map(|i| hash(1, i % W, i / W)).collect() };
        let mut world = Self { items: vec![base], pointer: (40, 40), serial: 1, square };
        for _ in 0..6 {
            world.open(rng);
        }
        world
    }

    /// Whether the planner defers nothing in this order: no opaque layer above a glass.
    fn in_order(&self, items: &[Item]) -> bool {
        self.square
            || items
                .iter()
                .position(Item::glass)
                .is_none_or(|g| !items[g..].iter().any(|i| i.layer.opaque))
    }

    fn item(&mut self, rng: &mut Rng) -> Item {
        self.serial += 1;
        let kind = rng.below(10);
        // Where writes are to be put off, opaque layers are many and large enough to hide a
        // glass.
        let (glass, opaque) = (kind < 3, (3..if self.square { 7 } else { 5 }).contains(&kind));
        let (w, h) = match (glass, self.square) {
            (true, _) => (24 + 2 * rng.below(29), 16 + 2 * rng.below(17)),
            (false, false) => (20 + rng.below(120), 16 + rng.below(84)),
            (false, true) => (60 + rng.below(160), 50 + rng.below(120)),
        };
        let layer = FrameLayer {
            src_row_abs: 1000 + self.serial,
            width: w,
            height: h,
            dst_x: rng.below(W - w + 1),
            dst_y: rng.below(H - h + 1),
            opacity: 255,
            corner_radius: if glass && !self.square { w.min(h) / 2 } else { 0 },
            backdrop_blur: if glass { ASKED } else { 0 },
            // Every glass is a window of its own; the layers under them belong to some window
            // or to none.
            layer_id: if glass || rng.chance(50) { self.serial } else { 0 },
            opaque,
            key: if rng.chance(80) { self.serial << 8 } else { 0 },
            ..FrameLayer::EMPTY
        };
        let content = (0..w * h).map(|i| hash(self.serial, i % w, i / w)).collect();
        Item { layer, content }
    }

    fn open(&mut self, rng: &mut Rng) -> bool {
        let (item, place) = (self.item(rng), 1 + rng.below(self.items.len() as u32) as usize);
        let mut items = self.items.clone();
        items.insert(place, item);
        let fits = self.in_order(&items);
        if fits {
            self.items = items;
        }
        fits
    }

    /// The content of a random part of a random layer changes: the rectangle a present names
    /// for it (a little wider than the change, as a compositor's union is) and its origin —
    /// the layer's key, or unknown.
    fn change(&mut self, rng: &mut Rng) -> Command {
        self.serial += 1;
        let k = rng.below(self.items.len() as u32) as usize;
        let item = &mut self.items[k];
        let (x, y, w, h) = item.rect();
        let (lx, ly) = (rng.below(w), rng.below(h));
        let (lw, lh) = (1 + rng.below((w - lx).min(40)), 1 + rng.below((h - ly).min(30)));
        for cy in ly..ly + lh {
            for cx in lx..lx + lw {
                item.content[(cy * w + cx) as usize] = hash(self.serial, cx, cy);
            }
        }
        let pad = rng.below(3);
        let rect = TileRect {
            x: (x + lx).saturating_sub(pad),
            y: (y + ly).saturating_sub(pad),
            width: lw + 2 * pad,
            height: lh + 2 * pad,
        };
        let origin = match (item.layer.key, rng.below(10)) {
            (0, 0..=8) | (_, 8) => 0,
            (key, 0..=7) => key,
            _ => 0xDEAD_0000 + self.serial,
        };
        Command::Damage { rect, origin }
    }

    /// The next present, and what it does (for a failure's message).
    fn present(&mut self, rng: &mut Rng, first: bool) -> (Vec<Command>, String) {
        let mut damage = Vec::new();
        let (mut layers, mut what) = (first || rng.chance(15), String::new());
        let n = self.items.len() as u32;
        let pick = |rng: &mut Rng| 1 + rng.below(n - 1) as usize;
        match rng.below(10) {
            0 if n < 10 => {
                layers = true;
                what = format!("open {}", self.open(rng));
            }
            1 if n > 4 => {
                layers = true;
                let k = pick(rng);
                self.items.remove(k);
                what = format!("close {k}");
            }
            2 => {
                layers = true;
                let k = pick(rng);
                let (_, _, w, h) = self.items[k].rect();
                let l = &mut self.items[k].layer;
                (l.dst_x, l.dst_y) = (rng.below(W - w + 1), rng.below(H - h + 1));
                what = format!("move {k}");
            }
            3 => {
                let (a, b) = (pick(rng), pick(rng));
                let mut items = self.items.clone();
                items.swap(a, b);
                if self.in_order(&items) {
                    layers = true;
                    self.items = items;
                    what = format!("swap {a} {b}");
                }
            }
            4 => {
                let old = self.pointer;
                self.pointer = (rng.below(W - POINTER), rng.below(H - POINTER));
                for (x, y) in [old, self.pointer] {
                    let rect = TileRect { x, y, width: POINTER, height: POINTER };
                    damage.push(Command::Damage { rect, origin: ORIGIN_ABOVE });
                }
                what = "pointer".into();
            }
            5 => {
                // A full present: layers and no damage named — its changes need no name.
                for _ in 0..3 {
                    self.change(rng);
                }
                return (self.items.iter().map(Item::command).collect(), "full".into());
            }
            _ => {}
        }
        for _ in 0..rng.below(4) {
            damage.push(self.change(rng));
        }
        what += &format!(" +{} named", damage.len());
        let mut cmds: Vec<Command> =
            if layers { self.items.iter().map(Item::command).collect() } else { Vec::new() };
        if damage.is_empty() {
            damage.push(Command::Damage { rect: TileRect::NONE, origin: 0 });
        }
        cmds.extend(damage);
        (cmds, what)
    }

    fn overlay(&self, img: &mut [u32]) {
        for y in 0..POINTER {
            for x in 0..POINTER {
                img[at(self.pointer.0 + x, self.pointer.1 + y)] = hash(3, x, y);
            }
        }
    }

    /// The full redraw: every glass blurred from what lies below it now.
    fn truth(&self) -> Vec<u32> {
        let mut img = wallpaper();
        for item in &self.items {
            let back = item.glass().then(|| blur_of(&img, item.rect()));
            item.paint(&mut img, back.as_deref());
        }
        self.overlay(&mut img);
        img
    }
}

/// A glass's cache rows in the model: the blur they hold and the rectangle it was made for.
struct Rows {
    rect: Rect,
    blur: Vec<u32>,
}

/// The model executor: the planner, the cache rows by their first row, and the plane shown.
struct Exec {
    frame: Frame,
    rows: HashMap<u32, Rows>,
    plane: Vec<u32>,
    by: WritesBy,
    counts: (u32, u32, u32),
}

impl Exec {
    fn new() -> Self {
        let mut frame = Frame::new(4096);
        frame.glass.ready = true;
        let (rows, plane) = (HashMap::new(), vec![0; (W * H) as usize]);
        Self { frame, rows, plane, by: WritesBy::default(), counts: (0, 0, 0) }
    }

    /// A glass's back as its step says: blurred into its rows, or read from them.
    fn back(&mut self, img: &[u32], item: &Item, glass: Glass, exact: bool) -> Vec<u32> {
        let rect = item.rect();
        match glass.cache {
            CacheStep::Off => blur_of(img, rect),
            CacheStep::Write(y) => {
                let all = Area::rect(rect.0, rect.1, rect.2, rect.3);
                match glass.band {
                    Area::Full => {
                        self.rows.insert(y, Rows { rect, blur: blur_of(img, rect) });
                    }
                    Area::None => panic!("a write without a band"),
                    band => {
                        let rows = self.rows.get_mut(&y).expect("a band written into no rows");
                        assert_eq!(rows.rect, rect, "a band written into another glass's rows");
                        if let Area::Rect { x0, y0, x1, y1 } = band.grown(REACH).intersect(all) {
                            blur_into(img, rect, (x0, y0, x1 - x0, y1 - y0), &mut rows.blur);
                        }
                    }
                }
                self.rows[&y].blur.clone()
            }
            CacheStep::Read(y) => {
                let rows = self.rows.get(&y).expect("a read of rows never written");
                if exact {
                    assert_eq!(rows.rect, rect, "a read of rows written for another place");
                }
                // Rows put off under what hides them may hold another size: drawn as they are.
                let (w, h) = (rows.rect.2, rows.rect.3);
                (0..rect.2 * rect.3)
                    .map(|i| {
                        rows.blur[((i / rect.2).min(h - 1) * w + (i % rect.2).min(w - 1)) as usize]
                    })
                    .collect()
            }
        }
    }

    /// One present: planned, drawn as planned, the plane redrawn inside the plan's damage.
    /// What the executor's rows give everywhere.
    fn show(&mut self, world: &World, cmds: &[Command], exact: bool) -> Vec<u32> {
        self.frame.layers.take_frame(cmds);
        let planned = self.frame.plan(DISPLAY, &|_| true);
        assert_eq!(planned.layers, world.items.len());
        let mut img = wallpaper();
        for (i, item) in world.items.iter().enumerate() {
            let step = self.frame.step(i).expect("a step for every layer");
            assert_eq!(step.layer, item.layer);
            let back = item.glass().then(|| {
                let glass = step.glass.expect("a glass step for every glass");
                self.counts.0 += u32::from(matches!(glass.cache, CacheStep::Read(_)));
                self.back(&img, item, glass, exact)
            });
            item.paint(&mut img, back.as_deref());
        }
        world.overlay(&mut img);
        if let Some((x0, y0, x1, y1)) = planned.damage.on(DISPLAY) {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    self.plane[at(x, y)] = img[at(x, y)];
                }
            }
        }
        let (by, sum) = (planned.by, &mut self.by);
        for (acc, n) in [
            (&mut sum.below, by.below),
            (&mut sum.glass, by.glass),
            (&mut sum.unknown, by.unknown),
            (&mut sum.moved, by.moved),
            (&mut sum.order, by.order),
        ] {
            *acc += n;
        }
        self.counts.1 += planned.writes.total();
        self.counts.2 += u32::from(planned.writes.hidden);
        img
    }
}

fn differs(a: &[u32], b: &[u32]) -> Option<(u32, u32)> {
    a.iter().zip(b).position(|(p, q)| p != q).map(|i| (i as u32 % W, i as u32 / W))
}

/// `FRAMES` presents of the sequence of `seed`; the executor at its end.
fn run(seed: u32, square: bool) -> Exec {
    let mut rng = Rng(seed);
    let (mut world, mut exec) = (World::new(&mut rng, square), Exec::new());
    let mut log = Vec::new();
    for frame in 0..FRAMES {
        let (cmds, what) = world.present(&mut rng, frame == 0);
        log.push(what);
        let tail = &log[log.len().saturating_sub(4)..];
        let drawn = exec.show(&world, &cmds, !square);
        let stale = differs(&exec.plane, &drawn);
        assert_eq!(stale, None, "frame {frame}: a changed pixel outside the damage {tail:?}");
        if !square {
            let wrong = differs(&drawn, &world.truth());
            assert_eq!(wrong, None, "frame {frame}: rows that miss a change {tail:?}");
        }
    }
    exec
}

#[test]
fn a_planned_frame_shows_what_a_full_redraw_shows() {
    for seed in [0x1234_5678, 0x9e37_79b9] {
        let exec = run(seed, false);
        let ((reads, writes, hidden), by) = (exec.counts, exec.by);
        println!("seed {seed:#x}: {reads} reads, {writes} writes, by {by:?}");
        // The sequence asked the rules: glasses read their rows through changes that were not
        // theirs, and were written for named layers below, for glasses below, for moves and
        // for swaps.
        assert!(reads >= 50 && writes >= 50, "{reads} reads, {writes} writes");
        assert!(by.below > 0 && by.glass > 0 && by.unknown > 0, "{by:?}");
        assert!(by.moved > 0 && by.order > 0, "{by:?}");
        assert_eq!(hidden, 0, "nothing is put off in this order");
    }
}

#[test]
fn a_planned_frame_redraws_every_pixel_it_changes_where_writes_are_put_off() {
    let exec = run(0x9e37_79b9, true);
    let ((reads, writes, hidden), by) = (exec.counts, exec.by);
    println!("{reads} reads, {writes} writes, {hidden} put off, by {by:?}");
    assert!(reads >= 50 && writes >= 50 && hidden > 0, "{reads} {writes} {hidden}");
}
