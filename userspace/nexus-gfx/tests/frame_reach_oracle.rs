// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The planner against a full redraw (TASK-0351 P6): 200 random presents — rectangles named with
//! their layer's key, with origin 0, with a key the set does not hold and from above; layers
//! that move, swap their order, open and close; windows moved by their transform (a slide that
//! returns or drops its layer where it went, a parked offset, a scale-in, a fade); full
//! presents — drawn by a model executor that does what a plan says and nothing else: it blurs a
//! glass's REST rectangle (`Glass::at`) into its rows where the step writes (the band grown by
//! the reach, or all), draws them spread over the rectangle the layer is drawn at, and redraws
//! the plane inside the plan's damage alone.
//!
//! - DAMAGE: the plane, redrawn inside each plan's damage only, is what the executor would draw
//!   everywhere — every pixel a frame changes lies in its damage. Held for any scene.
//! - ROWS: a glass's rows are the blur of what is shown below its rest rectangle, byte for byte
//!   — unless it is HELD (its window moves, or a layer moves below it within its reach), then
//!   for at most `HOLD_MAX` plans and, at rest over a mover, a blur radius of its travel. With
//!   no stale rows the frame is a full redraw's: always when nothing moves — the plan that ends
//!   a hold pays what it owed. Held where no write is put off (pill glasses, each a window of
//!   its own, over every opaque layer); the last sequence (square glasses, any order) holds
//!   DAMAGE alone.
//!
//! The model: a pixel is a number; an opaque layer replaces it, a translucent one mixes its
//! content over it, a glass over the sum of what lies below within `BLUR` pixels (a box — any
//! change in it changes the sum); a faded layer lets what is below through.

use nexus_gfx::command::buffer::{Command, LAYER_OPAQUE};
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::layers::ORIGIN_ABOVE;
use nexus_gfx::frame::{Area, CacheStep, Frame, FrameLayer, Glass, Transform, WritesBy};
use std::collections::HashMap;
use std::num::Wrapping;

const W: u32 = 256;
const H: u32 = 192;
const DISPLAY: (u32, u32) = (W, H);
/// The planner's reach of a glass (`frame::reach::GLASS_REACH`): what a band write blurs again.
const REACH: u32 = 66;
/// The model's box blur — nearly the reach: a change two blurs away (through a glass) is felt.
const BLUR: u32 = 60;
/// The blur a glass asks for (its rows follow it, the model's box does not) — the travel of a
/// mover a glass at rest is held through — and the plans a glass is held (`hold::HOLD_MAX`).
const ASKED: u32 = 40;
const HOLD_MAX: u32 = 64;
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
fn rect_of(l: &FrameLayer) -> Rect {
    (l.dst_x, l.dst_y, l.width, l.height)
}
fn area(r: Rect) -> Area {
    Area::rect(r.0, r.1, r.2, r.3)
}
fn wallpaper() -> Vec<u32> {
    (0..W * H).map(|i| hash(7, i % W, i / W)).collect()
}

/// The box sums of `img` around every pixel of `region`, into `rect`'s grid `out` (row by row).
fn blur_into(img: &[u32], rect: Rect, region: Rect, out: &mut [u32]) {
    let stride = (W + 1) as usize;
    let mut sums = vec![Wrapping(0u32); stride * (H + 1) as usize];
    for y in 0..H as usize {
        let mut row = Wrapping(0u32);
        for x in 0..W as usize {
            row += img[y * W as usize + x];
            sums[(y + 1) * stride + x + 1] = sums[y * stride + x + 1] + row;
        }
    }
    let sum = |x: u32, y: u32| sums[y as usize * stride + x as usize];
    for y in region.1..region.1 + region.3 {
        for x in region.0..region.0 + region.2 {
            let (x0, y0) = (x.saturating_sub(BLUR), y.saturating_sub(BLUR));
            let (x1, y1) = ((x + BLUR + 1).min(W), (y + BLUR + 1).min(H));
            let boxed = sum(x1, y1) - sum(x0, y1) - sum(x1, y0) + sum(x0, y0);
            out[((y - rect.1) * rect.2 + (x - rect.0)) as usize] = boxed.0;
        }
    }
}

fn blur_of(img: &[u32], rect: Rect) -> Vec<u32> {
    let mut out = vec![0; (rect.2 * rect.3) as usize];
    blur_into(img, rect, rect, &mut out);
    out
}

/// A grid of `gw` × `gh` spread over `w` × `h` pixels (the nearest cell; itself at its size).
fn spread(grid: &[u32], (gw, gh): (u32, u32), (w, h): (u32, u32)) -> Vec<u32> {
    let cell = |i: u32| (i / w * gh / h).min(gh - 1) * gw + (i % w * gw / w).min(gw - 1);
    (0..w * h).map(|i| grid[cell(i) as usize]).collect()
}

/// A window's motion by its transform, `of` steps and then at rest. `kind`: 0 a slide that
/// returns, 1 one that drops its layer where it went, 2 a parked offset, 3 a scale-in, 4 a fade.
#[derive(Clone, Copy)]
struct Motion {
    id: u32,
    kind: u32,
    step: i32,
    of: i32,
    d: (i32, i32),
    origin: (i32, i32),
}

impl Motion {
    fn transform(&self) -> Transform {
        let (s, t, (dx, dy)) = (self.step, Transform::IDENTITY, (self.d.0 as i16, self.d.1 as i16));
        match self.kind {
            0 | 1 => Transform { dx: dx * s as i16, dy: dy * s as i16, ..t },
            2 => Transform { dx, dy, ..t },
            3 => {
                let origin = (self.origin.0 as i16, self.origin.1 as i16);
                Transform { scale: (10_000 - 60 * (self.of + 1 - s)) as u16, origin, ..t }
            }
            _ => Transform { opacity: (255 - 40 * s.min(self.of + 1 - s)) as u8, ..t },
        }
    }

    fn command(&self, rest: bool) -> Command {
        let t = if rest { Transform::IDENTITY } else { self.transform() };
        let (dx, dy, opacity, scale) = (t.dx.into(), t.dy.into(), t.opacity.into(), t.scale.into());
        let (origin_x, origin_y) = (t.origin.0.into(), t.origin.1.into());
        Command::LayerTransform { layer_id: self.id, dx, dy, opacity, scale, origin_x, origin_y }
    }
}

/// One layer of the scene: what the present sends, and the model's content of its pixels.
#[derive(Clone)]
struct Item {
    layer: FrameLayer,
    content: Vec<u32>,
}

impl Item {
    fn glass(&self) -> bool {
        self.layer.backdrop_blur != 0
    }

    #[rustfmt::skip] // every field of the layer, one to one
    fn command(&self) -> Command {
        let l = &self.layer;
        Command::CompositeLayer {
            src_row_abs: l.src_row_abs, src_x: l.src_x, width: l.width, height: l.height,
            dst_x: l.dst_x, dst_y: l.dst_y, opacity: l.opacity, corner_radius: l.corner_radius,
            shadow_blur: l.shadow_blur, shadow_offset_y: l.shadow_offset_y,
            shadow_alpha: l.shadow_alpha, backdrop_blur: l.backdrop_blur, content_w: l.content_w,
            content_h: l.content_h, scroll_band_top_abs: l.scroll_band_top_abs,
            scroll_band_h: l.scroll_band_h, layer_id: l.layer_id, content_epoch: l.content_epoch,
            flags: if l.opaque { LAYER_OPAQUE } else { 0 }, key: l.key, source: l.source,
        }
    }

    /// The layer drawn over `img` as `l` (transformed, its content spread), a glass over `back`.
    fn paint(&self, img: &mut [u32], l: &FrameLayer, back: Option<&[u32]>) {
        let ((x0, y0, w, h), (cw, ch)) = (rect_of(l), (self.layer.width, self.layer.height));
        for (ly, lx) in (0..h).flat_map(|ly| (0..w).map(move |lx| (ly, lx))) {
            if x0 + lx >= W || y0 + ly >= H {
                continue;
            }
            let (i, p) = ((ly * w + lx) as usize, at(x0 + lx, y0 + ly));
            let content = self.content[((ly * ch / h) * cw + lx * cw / w) as usize];
            let solid = match back {
                Some(back) => mix(back[i], content),
                None if l.opaque => content,
                None => mix(img[p], content),
            };
            img[p] = if l.opacity == 255 { solid } else { mix(img[p] ^ l.opacity, solid) };
        }
    }
}

struct World {
    items: Vec<Item>,
    motions: Vec<Motion>,
    pointer: (u32, u32),
    serial: u32,
    /// Square glasses in any order (writes are put off), else pills above every opaque layer;
    /// `park`: the plans a parked offset stays (`0`: no window moves by its transform).
    square: bool,
    park: u32,
}

impl World {
    fn new(rng: &mut Rng, square: bool, park: u32) -> Self {
        let mut layer = FrameLayer { width: W, height: H, opacity: 255, ..FrameLayer::EMPTY };
        (layer.src_row_abs, layer.opaque, layer.key) = (1000, true, 0xFFFF_0000);
        let base = Item { layer, content: (0..W * H).map(|i| hash(1, i % W, i / W)).collect() };
        let (items, motions) = (vec![base], Vec::new());
        let mut world = Self { items, motions, pointer: (40, 40), serial: 1, square, park };
        for _ in 0..6 {
            world.open(rng);
        }
        world
    }

    /// Whether the planner defers nothing in this order: no opaque layer above a glass.
    fn in_order(&self, items: &[Item]) -> bool {
        self.square || !items.iter().skip_while(|i| !i.glass()).any(|i| i.layer.opaque)
    }

    fn moving(&self, id: u32) -> bool {
        id != 0 && self.motions.iter().any(|m| m.id == id)
    }

    /// `item`'s layer as drawn: its window's transform applied.
    fn drawn(&self, item: &Item) -> FrameLayer {
        let mut l = item.layer;
        if let Some(m) = self.motions.iter().find(|m| m.id == l.layer_id && l.layer_id != 0) {
            m.transform().apply(&mut l);
        }
        l
    }

    fn item(&mut self, rng: &mut Rng) -> Item {
        self.serial += 1;
        let kind = rng.below(10);
        // Where writes are to be put off, opaque layers are many and large enough to hide one.
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
            // Every glass is a window of its own, and named: a hold follows a layer's key.
            layer_id: if glass || rng.chance(50) { self.serial } else { 0 },
            opaque,
            key: if glass || rng.chance(80) { self.serial << 8 } else { 0 },
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
        self.items = if fits { items } else { std::mem::take(&mut self.items) };
        fits
    }

    /// A named window at rest starts to move (window `id`, else any) if it stays on screen.
    fn start(&mut self, rng: &mut Rng, id: Option<u32>) {
        let free = |i: &&Item| i.layer.key != 0 && i.layer.layer_id != 0;
        let still: Vec<&Item> = self.items.iter().filter(free).collect();
        let pick = still.get(rng.below(still.len().max(1) as u32) as usize).copied();
        let named = |id: u32| still.iter().copied().find(|i| i.layer.layer_id == id);
        let (kind, of) = (rng.below(5), 3 + rng.below(6) as i32);
        let d = (rng.below(17) as i32 - 8, rng.below(17) as i32 - 8);
        let Some(item) = id.map_or(pick, named).filter(|i| !self.moving(i.layer.layer_id)) else {
            return;
        };
        let (kind, (x, y, w, h)) =
            (if id.is_some() { 3 + kind % 2 } else { kind }, rect_of(&item.layer));
        let (of, far) = if kind == 2 { (self.park as i32, 1) } else { (of, of) };
        let (to_x, to_y) = (x as i32 + d.0 * far, y as i32 + d.1 * far);
        let on_screen =
            (0..=(W - w) as i32).contains(&to_x) && (0..=(H - h) as i32).contains(&to_y);
        if kind >= 3 || (d != (0, 0) && on_screen) {
            let origin = ((x + w / 2) as i32, y as i32);
            self.motions.push(Motion { id: item.layer.layer_id, kind, step: 1, of, d, origin });
        }
    }

    /// The content of a random part of a random layer changes: the rectangle a present names
    /// where the layer is drawn (a little wider, as a compositor's union is) and its origin.
    fn change(&mut self, rng: &mut Rng) -> Command {
        self.serial += 1;
        let k = rng.below(self.items.len() as u32) as usize;
        let (dx, dy, dw, dh) = rect_of(&self.drawn(&self.items[k]));
        let item = &mut self.items[k];
        let (w, h) = (item.layer.width, item.layer.height);
        let (lx, ly) = (rng.below(w), rng.below(h));
        let (lw, lh) = (1 + rng.below((w - lx).min(40)), 1 + rng.below((h - ly).min(30)));
        for cy in ly..ly + lh {
            for cx in lx..lx + lw {
                item.content[(cy * w + cx) as usize] = hash(self.serial, cx, cy);
            }
        }
        let pad = rng.below(3);
        let (x0, x1) = (dx + lx * dw / w, dx + ((lx + lw) * dw).div_ceil(w) + pad);
        let (y0, y1) = (dy + ly * dh / h, dy + ((ly + lh) * dh).div_ceil(h) + pad);
        let (x, y) = (x0.saturating_sub(pad), y0.saturating_sub(pad));
        let rect = TileRect { x, y, width: x1 + pad - x0, height: y1 + pad - y0 };
        let origin = match (item.layer.key, rng.below(10)) {
            (0, 0..=8) | (_, 8) => 0,
            (key, 0..=7) => key,
            _ => 0xDEAD_0000 + self.serial,
        };
        Command::Damage { rect, origin }
    }

    /// The next present, and what it does (for a failure's message).
    fn present(&mut self, rng: &mut Rng, first: bool) -> (Vec<Command>, String) {
        let (mut damage, mut cmds, mut full) = (Vec::new(), Vec::new(), false);
        let mut layers = first || rng.chance(15);
        // The motions step; one past its steps rests (a slide may drop its layer there).
        let stepped = self.motions.iter().map(|m| Motion { step: m.step + 1, ..*m });
        let (going, rested): (Vec<_>, Vec<_>) = stepped.partition(|m| m.step <= m.of);
        self.motions = going;
        for m in &rested {
            cmds.push(m.command(true));
            let dropped = self.items.iter_mut().find(|i| m.kind == 1 && i.layer.layer_id == m.id);
            if let Some(l) = dropped.map(|i| &mut i.layer) {
                l.dst_x = (l.dst_x as i32 + m.d.0 * m.of) as u32;
                l.dst_y = (l.dst_y as i32 + m.d.1 * m.of) as u32;
                layers = true;
            }
        }
        if self.park != 0 && rng.chance(30) {
            self.start(rng, None);
        }
        let n = self.items.len() as u32;
        let pick = |rng: &mut Rng| 1 + rng.below(n - 1) as usize;
        let case = rng.below(10);
        match case {
            0 if n < 10 => {
                (layers, _) = (true, self.open(rng));
                if self.park != 0 && rng.chance(50) {
                    // Born in motion: a panel's first frame is scaled or faded.
                    self.start(rng, Some(self.serial));
                }
            }
            1 if n > 4 => {
                layers = true;
                let gone = self.items.remove(pick(rng)).layer.layer_id;
                self.motions.retain(|m| m.id != gone);
            }
            2 => {
                let k = pick(rng);
                let l = self.items[k].layer;
                if !self.moving(l.layer_id) {
                    let to = (rng.below(W - l.width + 1), rng.below(H - l.height + 1));
                    (self.items[k].layer.dst_x, self.items[k].layer.dst_y, layers) =
                        (to.0, to.1, true);
                }
            }
            3 => {
                let (a, b) = (pick(rng), pick(rng));
                let mut items = self.items.clone();
                items.swap(a, b);
                if self.in_order(&items) {
                    (layers, self.items) = (true, items);
                }
            }
            4 => {
                let old = self.pointer;
                self.pointer = (rng.below(W - POINTER), rng.below(H - POINTER));
                for (x, y) in [old, self.pointer] {
                    let rect = TileRect { x, y, width: POINTER, height: POINTER };
                    damage.push(Command::Damage { rect, origin: ORIGIN_ABOVE });
                }
            }
            // A full present, its changes unnamed (it ends every hold: none where one is to
            // reach its bound).
            5 if self.park < HOLD_MAX => (full, layers) = (true, true),
            _ => {}
        }
        for _ in 0..if full { 3 } else { rng.below(4) } {
            damage.push(self.change(rng));
        }
        let what = format!("case {case} +{} named, {} moving", damage.len(), self.motions.len());
        cmds.extend(self.motions.iter().map(|m| m.command(false)));
        if layers {
            cmds.extend(self.items.iter().map(Item::command));
        }
        if damage.is_empty() {
            damage.push(Command::Damage { rect: TileRect::NONE, origin: 0 });
        }
        cmds.extend(damage.into_iter().filter(|_| !full));
        (cmds, what)
    }

    fn overlay(&self, img: &mut [u32]) {
        for (x, y) in (0..POINTER).flat_map(|y| (0..POINTER).map(move |x| (x, y))) {
            img[at(self.pointer.0 + x, self.pointer.1 + y)] = hash(3, x, y);
        }
    }

    /// The full redraw: every glass blurred from what lies below its rest rectangle now.
    fn truth(&self) -> Vec<u32> {
        let mut img = wallpaper();
        for item in &self.items {
            let (l, rect) = (self.drawn(item), rect_of(&item.layer));
            let blur = item.glass().then(|| blur_of(&img, rect));
            let back = blur.map(|b| spread(&b, (rect.2, rect.3), (l.width, l.height)));
            item.paint(&mut img, &l, back.as_deref());
        }
        self.overlay(&mut img);
        img
    }
}

/// The model executor: the planner, the cache rows by first row (their rectangle, their blur),
/// the plane shown; by a layer's source row where it was drawn last and its stale rows (plans
/// in a row, travel of the movers below); reads, writes, put off, stale draws, the longest run.
struct Exec {
    frame: Frame,
    rows: HashMap<u32, (Rect, Vec<u32>)>,
    plane: Vec<u32>,
    was: HashMap<u32, Rect>,
    debts: HashMap<u32, (u32, u32)>,
    by: WritesBy,
    counts: (u32, u32, u32, u32, u32),
}

impl Exec {
    fn new() -> Self {
        let mut frame = Frame::new(4096);
        frame.glass.ready = true;
        let (rows, plane) = (HashMap::new(), vec![0; (W * H) as usize]);
        let (was, debts, by) = (HashMap::new(), HashMap::new(), WritesBy::default());
        Self { frame, rows, plane, was, debts, by, counts: (0, 0, 0, 0, 0) }
    }

    /// A glass's rows as its step says, written or read: their first row (`None`: blurred live).
    fn rows(&mut self, img: &[u32], rect: Rect, glass: Glass, exact: bool) -> Option<u32> {
        assert_eq!(glass.at, rect, "a glass's rows are of its rest rectangle");
        let (CacheStep::Write(y) | CacheStep::Read(y)) = glass.cache else { return None };
        match (glass.cache, glass.band) {
            (CacheStep::Write(_), Area::Full) => {
                self.rows.insert(y, (rect, blur_of(img, rect)));
            }
            (CacheStep::Write(_), band) => {
                assert_ne!(band, Area::None, "a write without a band");
                let rows = self.rows.get_mut(&y).expect("a band written into no rows");
                assert_eq!(rows.0, rect, "a band written into another glass's rows");
                if let Area::Rect { x0, y0, x1, y1 } = band.grown(REACH).intersect(area(rect)) {
                    blur_into(img, rect, (x0, y0, x1 - x0, y1 - y0), &mut rows.1);
                }
            }
            _ => {
                let rows = self.rows.get(&y).expect("a read of rows never written");
                assert!(!exact || rows.0 == rect, "a read of rows written for another place");
            }
        }
        Some(y)
    }

    /// Glass `i`'s rows are stale: only a held glass's may be, within the hold's bounds.
    fn stale(&mut self, world: &World, i: usize) {
        let l = world.items[i].layer;
        let (own, reach) = (world.moving(l.layer_id), area(rect_of(&l)).grown(REACH));
        let below = world.items[..i].iter().filter(|b| b.layer.layer_id != l.layer_id);
        let over = below.filter(|b| world.moving(b.layer.layer_id)).filter_map(|b| {
            let (now, was) = (rect_of(&world.drawn(b)), self.was.get(&b.layer.src_row_abs));
            let edges = |r: Rect| [r.0, r.1, r.0 + r.2, r.1 + r.3];
            let shift =
                |w: &Rect| edges(now).iter().zip(edges(*w)).map(|(a, b)| a.abs_diff(b)).max();
            let near = area(now).touches(reach) || was.is_some_and(|w| area(*w).touches(reach));
            near.then(|| was.and_then(shift).unwrap_or(0))
        });
        let (over, debt) = (over.max(), self.debts.entry(l.src_row_abs).or_default());
        *debt = (debt.0 + 1, if own { 0 } else { debt.1 + over.unwrap_or(0) });
        assert!(own || over.is_some(), "stale rows of a glass nothing holds");
        assert!(debt.0 <= HOLD_MAX, "a glass held {} plans", debt.0);
        assert!(debt.1 <= ASKED, "a glass held through {} px of travel below it", debt.1);
        (self.counts.3, self.counts.4) = (self.counts.3 + 1, self.counts.4.max(debt.0));
    }

    /// One present, drawn as planned, the plane redrawn inside the plan's damage: what the
    /// rows give everywhere, and whether any glass drew stale rows.
    fn show(&mut self, world: &World, cmds: &[Command], exact: bool) -> (Vec<u32>, bool) {
        self.frame.layers.take_frame(cmds);
        let planned = self.frame.plan(DISPLAY, &|_| true);
        assert_eq!(planned.layers, world.items.len());
        let (mut img, mut any, mut was) = (wallpaper(), false, HashMap::new());
        for (i, item) in world.items.iter().enumerate() {
            let (step, l) =
                (self.frame.step(i).expect("a step for every layer"), world.drawn(item));
            assert_eq!(step.layer, l);
            was.insert(l.src_row_abs, rect_of(&l));
            let back = item.glass().then(|| {
                let glass = step.glass.expect("a glass step for every glass");
                self.counts.0 += u32::from(matches!(glass.cache, CacheStep::Read(_)));
                let (rect, fresh) = (rect_of(&item.layer), blur_of(&img, rect_of(&item.layer)));
                let held = self.rows(&img, rect, glass, exact).map(|y| &self.rows[&y]);
                // Rows put off under what hides them may hold another size: spread as they are.
                let (grid, of) = held.map_or((&fresh, rect), |rows| (&rows.1, rows.0));
                let (back, stale) =
                    (spread(grid, (of.2, of.3), (l.width, l.height)), *grid != fresh);
                if exact && stale {
                    any = true;
                    self.stale(world, i);
                } else if exact {
                    self.debts.remove(&l.src_row_abs);
                }
                back
            });
            item.paint(&mut img, &l, back.as_deref());
        }
        self.was = was;
        world.overlay(&mut img);
        if let Some((x0, y0, x1, y1)) = planned.damage.on(DISPLAY) {
            for (x, y) in (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))) {
                self.plane[at(x, y)] = img[at(x, y)];
            }
        }
        let (by, sum) = (planned.by, &mut self.by);
        (sum.below, sum.glass, sum.unknown) =
            (sum.below + by.below, sum.glass + by.glass, sum.unknown + by.unknown);
        (sum.moved, sum.order, sum.rest) =
            (sum.moved + by.moved, sum.order + by.order, sum.rest + by.rest);
        self.counts.1 += planned.writes.total();
        self.counts.2 += u32::from(planned.writes.hidden);
        (img, any)
    }
}

fn differs(a: &[u32], b: &[u32]) -> Option<(u32, u32)> {
    a.iter().zip(b).position(|(p, q)| p != q).map(|i| (i as u32 % W, i as u32 / W))
}

/// `FRAMES` presents of the sequence of `seed`; the executor at its end.
fn run(seed: u32, square: bool, park: u32) -> Exec {
    let mut rng = Rng(seed);
    let (mut world, mut exec) = (World::new(&mut rng, square, park), Exec::new());
    let mut log = Vec::new();
    for frame in 0..FRAMES {
        let (cmds, what) = world.present(&mut rng, frame == 0);
        log.push(what);
        let tail = &log[log.len().saturating_sub(4)..];
        let (drawn, stale) = exec.show(&world, &cmds, !square);
        let lost = differs(&exec.plane, &drawn);
        assert_eq!(lost, None, "frame {frame}: a changed pixel outside the damage {tail:?}");
        let wrong = (!square && !stale).then(|| differs(&drawn, &world.truth())).flatten();
        assert_eq!(wrong, None, "frame {frame}: rows that miss a change {tail:?}");
    }
    exec
}

#[test]
fn a_planned_frame_shows_what_a_full_redraw_shows() {
    for (seed, park) in [(0x1234_5678, 0), (0x9e37_79b9, 0), (0x1234_5678, 5), (0x9e37_79b9, 5)] {
        let exec = run(seed, false, park);
        let ((reads, writes, hidden, held, _), by) = (exec.counts, exec.by);
        println!("seed {seed:#x} park {park}: {reads} reads, {writes} writes, {held} held, {by:?}");
        // The sequence asked the rules: reads, writes by every cause, holds paid at rest.
        assert!(reads >= 50 && writes >= 50 - 20 * park.min(1), "{reads} reads, {writes} writes");
        assert!(by.below > 0 && by.unknown > 0 && by.moved > 0, "{by:?}");
        assert_eq!(hidden, 0, "nothing is put off in this order");
        let asked =
            if park == 0 { by.glass > 0 && by.order > 0 } else { held >= 20 && by.rest > 0 };
        assert!(asked, "{held} held, {by:?}");
    }
}

#[test]
fn test_reject_stale_rows_past_a_hold_s_bounds_under_parked_transforms() {
    let (_, writes, _, held, longest) = run(0x1234_5678, false, 80).counts;
    println!("{writes} writes, {held} held, the longest hold {longest} plans");
    assert_eq!(longest, HOLD_MAX, "the sequence holds a glass up to the bound");
}

#[test]
fn a_planned_frame_redraws_every_pixel_it_changes_where_writes_are_put_off() {
    for park in [0, 5] {
        let exec = run(0x9e37_79b9, true, park);
        let ((reads, writes, hidden, ..), by) = (exec.counts, exec.by);
        println!("park {park}: {reads} reads, {writes} writes, {hidden} put off, by {by:?}");
        assert!(reads >= 50 && writes >= 50 - 4 * park && hidden > 0, "{reads} {writes} {hidden}");
    }
}
