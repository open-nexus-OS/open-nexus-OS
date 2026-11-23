// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Runs against the frames they stand for (RFC-0109 "Runs and group opacity", TASK-0351 P19,
//! TASK-0354 P4), every pass of the board run on the host in exact arithmetic
//! (`frame_runs_exec.rs`):
//! - BELOW: over 200 random presents of the kinds the reach oracle sends — rectangles named
//!   with their layer's key, with origin 0, with a key the set does not hold and from above;
//!   layers that open, close, move and swap their order; windows moved, parked, scaled and
//!   faded by their transform; full presents — a frame drawn with the below run is the frame
//!   drawn without it, texel for texel, colour and alpha.
//! - GROUP: a fading group is desktop·(1 − a) + group·a; its run put back at opacity 255 is
//!   the frame its layers draw at rest.
//! - The rejects: a change inside a run is drawn before the run is read; a run never holds a
//!   layer that moves apart; a third run degrades to layers and is counted, no pass is
//!   refused; no layer's source reaches a run's plane.

#[path = "frame_runs_exec.rs"]
mod exec;
#[path = "frame_runs/random.rs"]
mod random;

use exec::{add, frac, mul, sub, Gpu, Texel, CACHE_ROWS, DISPLAY, HALF_ROWS, LAYOUT_ROWS, P};
use nexus_gfx::command::buffer::{Command, LAYER_OPAQUE};
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::layers::ORIGIN_ABOVE;
use nexus_gfx::frame::{
    Board, DrawSpec, Frame, FrameLayer, PassKind, Pipe, RunKind, RunLimits, RunSpec, RunsShort,
    Tex, Transform, RUN_TARGETS,
};
use random::random_present;
use std::collections::HashMap;

const W: u32 = DISPLAY.0;
const H: u32 = DISPLAY.1;
const FULL: (u32, u32, u32, u32) = (0, 0, W - 1, H - 1);
/// A layer's content in the layout: `BAND` rows from `ATLAS + band · BAND`.
const ATLAS: u32 = 1024;
const BAND: u32 = 256;
/// Every moving layer takes the below run; the board's number is its own.
const EAGER: RunLimits = RunLimits { targets: RUN_TARGETS, budget_px: 0, draws: usize::MAX };
type Rect = (u32, u32, u32, u32);

struct Rng(u32);

impl Rng {
    fn below(&mut self, n: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 % n
    }
}

fn hash(a: u32, b: u32, c: u32) -> u64 {
    let h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA6B) ^ c.wrapping_mul(0xC2B2_AE35);
    u64::from((h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D)) * 0x1_0001 % P
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Opaque,
    Sheer,
    Glass,
}

/// The scene: the layers a present sends, their content in the layout, the transforms.
#[derive(Clone)]
struct World {
    items: Vec<FrameLayer>,
    content: HashMap<u32, (u32, Vec<Texel>)>,
    moves: Vec<(u32, Transform)>,
    serial: u32,
}

impl World {
    /// A desktop: one opaque layer over the whole display.
    fn new() -> Self {
        let mut w =
            Self { items: Vec::new(), content: HashMap::new(), moves: Vec::new(), serial: 0 };
        let base = w.layer(Kind::Opaque, (0, 0, W, H), 0);
        w.items.push(base);
        w
    }

    /// A layer of `kind` at `rect` in window `id`, its content new in the layout: opaque
    /// texels, or straight ones at every quarter of alpha.
    fn layer(&mut self, kind: Kind, (x, y, w, h): Rect, id: u32) -> FrameLayer {
        self.serial += 1;
        let band = self.serial;
        let quarter = frac(1, 4);
        let texel = |i: u32| {
            let alpha = if kind == Kind::Opaque { 1 } else { mul(hash(band, i, 9) % 5, quarter) };
            [hash(band, i % w, i / w), alpha]
        };
        self.content.insert(band, (w, (0..w * h).map(texel).collect()));
        FrameLayer {
            src_row_abs: ATLAS + band * BAND,
            width: w,
            height: h,
            dst_x: x,
            dst_y: y,
            opacity: 255,
            backdrop_blur: if kind == Kind::Glass { 40 } else { 0 },
            layer_id: id,
            opaque: kind == Kind::Opaque,
            key: band << 8,
            ..FrameLayer::EMPTY
        }
    }

    fn add(&mut self, kind: Kind, rect: Rect, id: u32) -> usize {
        let l = self.layer(kind, rect, id);
        self.items.push(l);
        self.items.len() - 1
    }

    /// The layout's texel: the wallpaper's rows first, then each layer's band.
    fn texel(&self, x: u32, row: u32) -> Texel {
        if row < ATLAS {
            return [hash(7, x, row), 1];
        }
        let (band, r) = ((row - ATLAS) / BAND, (row - ATLAS) % BAND);
        let (w, texels) = &self.content[&band];
        texels[(r * w + x) as usize]
    }

    /// `n` texels of item `k` from `(lx, ly)` change; the rectangle a present names for it.
    fn change(&mut self, k: usize, (lx, ly, lw, lh): Rect, origin: u32) -> Command {
        self.serial += 1;
        let l = self.items[k];
        let band = (l.src_row_abs - ATLAS) / BAND;
        let texels = &mut self.content.get_mut(&band).expect("a band").1;
        for (cx, cy) in (ly..ly + lh).flat_map(|cy| (lx..lx + lw).map(move |cx| (cx, cy))) {
            texels[(cy * l.width + cx) as usize][0] = hash(self.serial, cx, cy);
        }
        let mut drawn = l;
        if let Some((_, t)) = self.moves.iter().find(|(id, _)| *id == l.layer_id && *id != 0) {
            t.apply(&mut drawn);
        }
        let rect = TileRect { x: drawn.dst_x + lx, y: drawn.dst_y + ly, width: lw, height: lh };
        Command::Damage { rect, origin }
    }

    #[rustfmt::skip] // every field of the layer, one to one
    fn command(l: &FrameLayer) -> Command {
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

    /// A present: every window's transform, the layers when `layers`, the damage — none
    /// named for `None` (with layers: everything), `TileRect::NONE` for an empty list.
    fn present(&self, layers: bool, damage: Option<&[Command]>) -> Vec<Command> {
        let transform = |(id, t): &(u32, Transform)| Command::LayerTransform {
            layer_id: *id,
            dx: t.dx.into(),
            dy: t.dy.into(),
            opacity: t.opacity.into(),
            scale: t.scale.into(),
            origin_x: t.origin.0.into(),
            origin_y: t.origin.1.into(),
        };
        let mut cmds: Vec<Command> = self.moves.iter().map(transform).collect();
        if layers {
            cmds.extend(self.items.iter().map(Self::command));
        }
        match damage {
            Some([]) => cmds.push(Command::Damage { rect: TileRect::NONE, origin: 0 }),
            Some(named) => cmds.extend_from_slice(named),
            None => assert!(layers, "a present without layers names its damage"),
        }
        cmds
    }

    fn fade(&mut self, id: u32, opacity: u8) {
        self.moves.retain(|(m, _)| *m != id);
        self.moves.push((id, Transform { opacity, ..Transform::IDENTITY }));
    }
}

/// An executor: a planner and its targets; with `limits` it takes runs.
struct Exec {
    frame: Frame,
    gpu: Gpu,
    limits: Option<RunLimits>,
    short: RunsShort,
    /// `Board::uncut_px` of the last frame's passes.
    uncut: u64,
}

impl Exec {
    fn new(limits: Option<RunLimits>) -> Self {
        let mut frame = Frame::new(CACHE_ROWS);
        frame.glass.ready = true;
        Self { frame, gpu: Gpu::new(), limits, short: RunsShort::default(), uncut: 0 }
    }

    /// One present drawn: planned, its runs decided, every pass of the board run over the
    /// whole display (a frame that changes nothing draws no pass). The runs it used.
    fn show(&mut self, world: &World, cmds: &[Command]) -> [Option<RunSpec>; RUN_TARGETS] {
        self.show_as(world, cmds, &|d| d)
    }

    fn show_as(
        &mut self,
        world: &World,
        cmds: &[Command],
        alter: &dyn Fn(DrawSpec) -> DrawSpec,
    ) -> [Option<RunSpec>; RUN_TARGETS] {
        self.frame.layers.take_frame(cmds);
        let planned = self.frame.plan(DISPLAY, &|_| true);
        assert_eq!(planned.layers, world.items.len());
        assert_eq!(self.frame.glass.short(), 0, "the model's one cache plane holds the glasses");
        let area = planned.damage.on(DISPLAY).map(|_| FULL);
        let rows = (LAYOUT_ROWS, HALF_ROWS);
        if let Some(limits) = self.limits {
            self.short = self.frame.take_runs(planned, (DISPLAY, rows), area, limits);
        }
        if area.is_some() {
            let board = Board::new(&self.frame, planned, DISPLAY, rows.0, rows.1);
            (self.gpu.begin(), self.uncut = 0);
            for p in 0..board.pass_count() {
                let spec = board.pass(p).expect("no pass of the plan is refused");
                self.uncut += board.uncut_px(&spec);
                assert_eq!(board.draws(&spec), board.walk(&spec).count(), "pass {p}");
                self.gpu.pass(&board, &spec, &|x, row| world.texel(x, row), alter);
            }
        }
        self.frame.runs()
    }
}

fn below(runs: &[Option<RunSpec>; RUN_TARGETS]) -> Option<RunSpec> {
    runs.iter().flatten().copied().find(|r| r.kind == RunKind::Below)
}

fn groups(runs: &[Option<RunSpec>; RUN_TARGETS]) -> Vec<RunSpec> {
    runs.iter().flatten().copied().filter(|r| r.kind != RunKind::Below).collect()
}

fn differs(a: &[Texel], b: &[Texel]) -> Option<(u32, u32)> {
    a.iter().zip(b).position(|(p, q)| p != q).map(|i| (i as u32 % W, i as u32 / W))
}

#[test]
fn a_frame_drawn_with_the_below_run_is_the_frame_drawn_without_it() {
    for seed in [0x1234_5678, 0x9e37_79b9] {
        let (mut rng, mut world, mut motions) = (Rng(seed), World::new(), Vec::new());
        for _ in 0..5 {
            let rect = (rng.below(W - 60), rng.below(H - 40), 60, 40);
            let kind = [Kind::Opaque, Kind::Sheer, Kind::Glass][rng.below(3) as usize];
            world.add(kind, rect, world.serial + 100);
        }
        // Byte for byte on the board's sampler, which clamps at a texture's rectangle (R-own).
        let (mut with, mut without) = (Exec::new(Some(EAGER)), Exec::new(None));
        // Frames that read the run: as it stood, a part of it drawn again, all of it.
        let (mut held, mut part, mut whole) = (0, 0, 0);
        for frame in 0..200 {
            let cmds = if frame == 0 {
                world.present(true, None)
            } else {
                random_present(&mut world, &mut motions, &mut rng)
            };
            let runs = with.show(&world, &cmds);
            assert!(without.show(&world, &cmds).iter().all(Option::is_none));
            assert!(groups(&runs).is_empty(), "every window is one layer: no group run");
            match below(&runs).map(|r| r.redraw) {
                Some(None) => held += 1,
                Some(Some(FULL)) => whole += 1,
                Some(Some(_)) => part += 1,
                None => {}
            }
            let lost = differs(&with.gpu.slot, &without.gpu.slot);
            assert_eq!(lost, None, "seed {seed:#x} frame {frame}: the run {:?}", below(&runs));
        }
        println!(
            "seed {seed:#x}: the below run read as held {held}, in part {part}, whole {whole}"
        );
        assert!(held >= 40 && part >= 40 && whole >= 10, "{held} held, {part} part, {whole} whole");
    }
}

/// A desktop, a window, the group of window 9 (a glass panel, an opaque pane and a sheer
/// label on it) and a layer above it: the scene, the group's first step.
fn panel() -> (World, usize) {
    let mut w = World::new();
    w.add(Kind::Sheer, (20, 10, 90, 70), 1);
    let first = w.add(Kind::Glass, (48, 24, 96, 64), 9);
    w.add(Kind::Opaque, (56, 32, 40, 28), 9);
    w.add(Kind::Sheer, (88, 44, 44, 32), 9);
    w.add(Kind::Sheer, (120, 60, 50, 40), 0);
    (w, first)
}

/// The frame of `world` drawn at once, without runs.
fn plain(world: &World) -> Vec<Texel> {
    let mut exec = Exec::new(None);
    exec.show(world, &world.present(true, None));
    exec.gpu.slot
}

#[test]
fn a_fading_group_is_the_group_formula_and_at_full_opacity_its_layers() {
    let (mut world, first) = panel();
    let rest = plain(&world);
    let mut bare = world.clone();
    bare.items.drain(first..first + 3);
    let desktop = plain(&bare);
    // The fade's frames: the group in one run from the first on, drawn once.
    let (mut exec, mut full) = (Exec::new(Some(EAGER)), Exec::new(Some(EAGER)));
    let opaque = |d: DrawSpec| match d.pipe {
        Pipe::Premultiplied => {
            let mut quad = d.quad;
            quad.push[..4].copy_from_slice(&[1.0; 4]);
            DrawSpec { quad, ..d }
        }
        _ => d,
    };
    for (frame, opacity) in [51u8, 153, 204].into_iter().enumerate() {
        world.fade(9, opacity);
        let cmds = world.present(frame == 0, (frame > 0).then_some(&[][..]));
        let runs = exec.show(&world, &cmds);
        let (g, a) = (groups(&runs), frac(u64::from(opacity), 255));
        assert_eq!(g.len(), 1, "{runs:?}");
        assert_eq!((g[0].kind, g[0].from, g[0].to), (RunKind::Group(9), first, first + 3));
        assert_eq!(
            (g[0].rect, g[0].at, g[0].opacity),
            ((48, 24, 96, 64), (48, 24, 96, 64), opacity.into())
        );
        assert_eq!(g[0].redraw.is_some(), frame == 0, "the group is drawn once");
        assert_eq!(below(&runs).map(|r| (r.to, r.redraw.is_some())), Some((first, frame == 0)));
        // desktop·(1 − a) + group·a, colour and alpha.
        let formula = |i: usize, c: usize| add(mul(desktop[i][c], sub(1, a)), mul(rest[i][c], a));
        let wrong = (0..rest.len()).find(|&i| exec.gpu.slot[i] != [formula(i, 0), formula(i, 1)]);
        assert_eq!(wrong.map(|i| (i as u32 % W, i as u32 / W)), None, "opacity {opacity}");
        // The same run put back at opacity 255 is the frame the layers draw at rest.
        full.show_as(&world, &cmds, &opaque);
        assert_eq!(differs(&full.gpu.slot, &rest), None, "opacity {opacity}");
    }
    // A group that moves while it fades is drawn back where its transform puts it.
    world.moves = vec![(9, Transform { dx: 8, dy: -4, opacity: 204, ..Transform::IDENTITY })];
    let runs = exec.show(&world, &world.present(false, Some(&[])));
    let g = groups(&runs);
    assert_eq!((g[0].at, g[0].rect, g[0].redraw), ((56, 20, 96, 64), (48, 24, 96, 64), None));
    // At rest again the group is its layers: no run reads the plane.
    world.moves = vec![(9, Transform::IDENTITY)];
    let runs = exec.show(&world, &world.present(false, Some(&[])));
    assert!(groups(&runs).is_empty() && below(&runs).is_none(), "{runs:?}");
    assert_eq!(differs(&exec.gpu.slot, &rest), None);
}

/// A desktop, two windows (the second of `kind`) and a third that slides over them.
fn sliding(kind: Kind) -> (World, [usize; 3]) {
    let mut w = World::new();
    let a = w.add(Kind::Sheer, (10, 10, 80, 60), 1);
    let b = w.add(kind, (100, 60, 60, 48), 2);
    let top = w.add(Kind::Sheer, (40, 30, 70, 50), 3);
    w.moves = vec![(3, Transform { dx: 4, dy: 2, ..Transform::IDENTITY })];
    (w, [a, b, top])
}

/// The same present drawn with runs and without: the frames are one; the below run read.
fn both(
    execs: &mut (Exec, Exec),
    world: &mut World,
    layers: bool,
    named: Option<&[Command]>,
) -> RunSpec {
    // The slide goes on: every frame draws.
    world.moves[0].1.dx += 1;
    let cmds = world.present(layers, named);
    let runs = execs.0.show(world, &cmds);
    execs.1.show(world, &cmds);
    assert_eq!(differs(&execs.0.gpu.slot, &execs.1.gpu.slot), None, "{runs:?}");
    below(&runs).expect("a layer moves: the below run is read")
}

#[test]
fn test_reject_a_run_drawn_after_a_change_inside_it() {
    let (mut world, [a, b, top]) = sliding(Kind::Opaque);
    let mut execs = (Exec::new(Some(EAGER)), Exec::new(None));
    // Taken whole under the layer that moves.
    let run = both(&mut execs, &mut world, true, None);
    assert_eq!((run.from, run.to, run.redraw), (0, top, Some(FULL)));
    // Nothing changed below: read as it stands.
    assert_eq!(both(&mut execs, &mut world, false, Some(&[])).redraw, None);
    // A change inside it, named with its layer's key: drawn before the slot's pass reads it.
    let key = world.items[a].key;
    let named = world.change(a, (5, 6, 20, 10), key);
    let redraw = both(&mut execs, &mut world, false, Some(&[named])).redraw;
    assert_eq!(redraw, Some((15, 16, 34, 25)));
    // One of unknown origin, and one of a key the set does not hold: the run's as well.
    for origin in [0, 0xDEAD_0000] {
        let named = world.change(b, (0, 0, 8, 8), origin);
        let redraw = both(&mut execs, &mut world, false, Some(&[named])).redraw;
        assert_eq!(redraw, Some((100, 60, 107, 67)), "origin {origin:#x}");
    }
    // The moving layer's own content and the pointer's are not the run's.
    let rect = TileRect { x: 3, y: 3, width: 8, height: 8 };
    let above = Command::Damage { rect, origin: ORIGIN_ABOVE };
    let key = world.items[top].key;
    let own = world.change(top, (2, 2, 30, 20), key);
    assert_eq!(both(&mut execs, &mut world, false, Some(&[own, above])).redraw, None);
    // A layer of it drawn elsewhere: where it was and where it is.
    world.items[a].dst_x += 12;
    let redraw = both(&mut execs, &mut world, true, Some(&[])).redraw;
    assert_eq!(redraw, Some((10, 10, 101, 69)));
    // A full present, and another list below it: taken again, whole.
    assert_eq!(both(&mut execs, &mut world, true, None).redraw, Some(FULL));
    world.items.remove(a);
    let run = both(&mut execs, &mut world, true, Some(&[]));
    assert_eq!((run.to, run.redraw), (top - 1, Some(FULL)));
    // A glass of the run written again (a change below it, within its reach) is drawn
    // again in the run with the change.
    let (mut world, [a, b, _]) = sliding(Kind::Glass);
    both(&mut execs, &mut world, true, None);
    let key = world.items[a].key;
    let named = world.change(a, (60, 50, 10, 6), key);
    let redraw = both(&mut execs, &mut world, false, Some(&[named])).redraw.expect("drawn again");
    let glass = world.items[b];
    assert!(redraw.0 <= 70 && redraw.1 <= 60, "the change: {redraw:?}");
    assert!(
        redraw.2 >= glass.dst_x + glass.width - 1 && redraw.3 >= glass.dst_y + glass.height - 1
    );
}

/// R-own, on the board's sampler (it clamps at a texture's rectangle): a glass back is cut
/// around an opaque layer of its OWN window alone, and the below run ends where no window lies
/// across — so the run's pass cuts every back as the slot's does. The scene that differed
/// along the cut (another window's layer above the run bites the glass) is one frame now; the
/// cost is counted: the back's pixels drawn under that layer.
#[test]
fn test_reject_a_glass_back_cut_around_another_window_s_layer() {
    let mut world = World::new();
    let glass = world.add(Kind::Glass, (40, 24, 96, 64), 1);
    world.add(Kind::Opaque, (72, 40, 60, 60), 2);
    world.moves = vec![(2, Transform { dx: 4, dy: 4, ..Transform::IDENTITY })];
    let mut execs = (Exec::new(Some(EAGER)), Exec::new(None));
    let cmds = world.present(true, None);
    assert_eq!(below(&execs.0.show(&world, &cmds)).map(|r| r.to), Some(glass + 1));
    execs.1.show(&world, &cmds);
    assert_eq!(differs(&execs.0.gpu.slot, &execs.1.gpu.slot), None);
    // The layer as drawn, (76, 44) on, lies over 60 x 44 of the glass: drawn, not cut out.
    assert_eq!((execs.0.uncut, execs.1.uncut), (0, 60 * 44), "of {} px of glass", 96 * 64);
    // The pane in the glass's own window cuts it (nothing drawn under it) — below the layer
    // that moves the run holds both; above it the window lies across: the run ends under it.
    for (across, to) in [(false, 3), (true, 1)] {
        let mut world = World::new();
        world.add(Kind::Glass, (40, 24, 96, 64), 1);
        world.add(Kind::Opaque, (72, 40, 60, 60), 1);
        world.add(Kind::Sheer, (30, 30, 70, 50), 3);
        world.items.swap(2, if across { 3 } else { 2 });
        world.moves = vec![(3, Transform { dx: 4, dy: 4, ..Transform::IDENTITY })];
        let mut execs = (Exec::new(Some(EAGER)), Exec::new(None));
        assert_eq!(both(&mut execs, &mut world, true, None).to, to);
        assert_eq!((execs.0.uncut, execs.1.uncut), (0, 0), "its own pane: cut");
        assert_eq!(both(&mut execs, &mut world, false, Some(&[])).redraw, None);
    }
}

#[test]
fn test_reject_a_run_holding_a_layer_that_moves_apart() {
    let (mut world, [a, _, top]) = sliding(Kind::Glass);
    let mut execs = (Exec::new(Some(EAGER)), Exec::new(None));
    assert_eq!(both(&mut execs, &mut world, true, None).to, top);
    // A layer inside the run starts to move: the run ends below it, drawn anew.
    world.moves.push((1, Transform { dx: -3, dy: 5, ..Transform::IDENTITY }));
    let run = both(&mut execs, &mut world, false, Some(&[]));
    assert_eq!((run.to, run.redraw), (a, Some(FULL)));
    // It rests again: the run keeps its shorter range (it never grows while held).
    world.moves[1].1 = Transform::IDENTITY;
    let run = both(&mut execs, &mut world, false, Some(&[]));
    assert_eq!((run.to, run.redraw), (a, None));
    // A fading window of two layers with another window's layer between them is not one
    // range: no run holds it, it is drawn as layers and counted.
    let (mut with, mut without) = execs;
    let mut show = |world: &World, cmds: &[Command]| {
        let runs = with.show(world, cmds);
        without.show(world, cmds);
        assert_eq!(differs(&with.gpu.slot, &without.gpu.slot), None, "{runs:?}");
        (runs, with.short)
    };
    let (mut world, first) = panel();
    world.items[first + 1].layer_id = 4;
    world.fade(9, 128);
    let (runs, short) = show(&world, &world.present(true, None));
    assert!(groups(&runs).is_empty(), "{runs:?}");
    assert_eq!(short, RunsShort { below: false, groups: 0, apart: 1 });
    // A group whose bounds at rest leave the display would lose what lies outside.
    let (mut world, first) = panel();
    world.items[first].dst_x = W - 40;
    world.fade(9, 128);
    let (runs, short) = show(&world, &world.present(true, None));
    assert!(groups(&runs).is_empty() && short.apart == 1, "{runs:?} {short:?}");
}

/// Three fading windows of two layers each, side by side.
fn three_groups() -> World {
    let mut w = World::new();
    for (i, x) in [8u32, 68, 128].into_iter().enumerate() {
        w.add(Kind::Sheer, (x, 20, 52, 60), 11 + i as u32);
        w.add(Kind::Opaque, (x + 6, 30, 30, 20), 11 + i as u32);
        w.fade(11 + i as u32, 128);
    }
    w
}

#[test]
fn test_reject_a_third_run() {
    let world = three_groups();
    let cmds = world.present(true, None);
    // Two targets: the two lowest groups fade as one each; the third and the below run find
    // none — counted, drawn as layers, and every pass of the frame is there.
    let mut exec = Exec::new(Some(EAGER));
    let runs = exec.show(&world, &cmds);
    let ids: Vec<RunKind> = groups(&runs).iter().map(|r| r.kind).collect();
    assert_eq!(ids, [RunKind::Group(11), RunKind::Group(12)]);
    assert_eq!(exec.short, RunsShort { below: true, groups: 1, apart: 0 });
    // The next frame of the fades holds what it held: nothing is drawn again or taken away.
    let mut world = world;
    (11..14).for_each(|id| world.fade(id, 160));
    let runs = exec.show(&world, &world.present(false, Some(&[])));
    assert!(groups(&runs).iter().all(|r| r.redraw.is_none()) && groups(&runs).len() == 2);
    // No target at all (the executor's memory refused them): every group as layers, the
    // frame the one a planner without runs draws.
    let mut none = Exec::new(Some(RunLimits { targets: 0, ..EAGER }));
    let mut without = Exec::new(None);
    assert!(none.show(&world, &cmds).iter().all(Option::is_none));
    without.show(&world, &cmds);
    assert_eq!(none.short, RunsShort { below: true, groups: 3, apart: 0 });
    assert_eq!(differs(&none.gpu.slot, &without.gpu.slot), None);
    // A below run under budget is not due and not counted.
    let mut calm = Exec::new(Some(RunLimits { targets: 0, budget_px: u64::MAX, ..EAGER }));
    calm.show(&world, &cmds);
    assert!(!calm.short.below);
    // A run whose own pass would ask more draws than a pass holds is not taken: counted.
    let mut tight = Exec::new(Some(RunLimits { draws: 2, ..EAGER }));
    assert!(tight.show(&world, &cmds).iter().all(Option::is_none));
    assert_eq!(tight.short, RunsShort { below: true, groups: 3, apart: 0 });
    assert_eq!(differs(&tight.gpu.slot, &without.gpu.slot), None);
}

#[test]
fn test_reject_a_run_as_a_layer_s_source() {
    // Layers that name every source a run's plane could be mistaken for — the targets'
    // numbers, the largest id — while a below run and a group run are in use.
    let (mut world, first) = panel();
    for (k, source) in [1u32, 2, RUN_TARGETS as u32, u32::MAX].into_iter().enumerate() {
        world.items[1 + k].source = source;
        world.items[1 + k].src_row_abs = 0;
    }
    world.fade(9, 128);
    let mut frame = Frame::new(CACHE_ROWS);
    frame.glass.ready = true;
    frame.layers.take_frame(&world.present(true, None));
    let planned = frame.plan(DISPLAY, &|_| true);
    frame.take_runs(planned, (DISPLAY, (LAYOUT_ROWS, HALF_ROWS)), Some(FULL), EAGER);
    assert_eq!(frame.runs().iter().flatten().count(), 2);
    let board = Board::new(&frame, planned, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let (mut from_runs, mut buffers) = (Vec::new(), 0);
    for spec in (0..board.pass_count()).filter_map(|p| board.pass(p)) {
        for d in board.walk(&spec) {
            match d.tex {
                Tex::Run { slot, row, .. } => from_runs.push((spec.kind, d.pipe, slot, row >= H)),
                Tex::Buffer { source, .. } => {
                    assert!([1, 2, RUN_TARGETS as u32, u32::MAX].contains(&source));
                    buffers += 1;
                }
                _ => {}
            }
        }
    }
    // A run's plane is sampled by the planner's own three draws and by nothing else: the
    // group's clear (the transparent rows), the below run and the group drawn back.
    let slot = PassKind::Scene { upto: world.items.len() };
    let rest = PassKind::Rest { from: first, to: first + 3 };
    let (s, g) = (below(&frame.runs()).expect("below").slot, groups(&frame.runs())[0].slot);
    assert_eq!(
        from_runs,
        [
            (rest, Pipe::Opaque, g, true),
            (slot, Pipe::Opaque, s, false),
            (slot, Pipe::Premultiplied, g, false)
        ]
    );
    assert!(buffers >= 4, "{buffers} draws of the layers' buffers");
}
