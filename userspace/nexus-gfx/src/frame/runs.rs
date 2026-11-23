// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Runs (RFC-0109 "Runs and group opacity", RFC-0101 §8; TASK-0351 P19, TASK-0354 P4,
//! TASK-0355 P2): a contiguous range of the plan's layers drawn once into a target of the
//! executor's own and drawn from it as one quad while it holds. A fade over three open windows
//! drew every window again each frame (17-24 ms on the board) though no frame changed them.
//! - The BELOW run is the wallpaper and the steps `[0, k)` under the lowest layer in motion: a
//!   plane that holds a prefix of the scene. Its `lack` is everything that changed the prefix
//!   since it was drawn; the lack is drawn again (a scene pass up to `k`, cut to it) before the
//!   slot's pass reads the plane with the opaque program, one to one.
//! - A GROUP run is the layers of one transform id at REST over transparent texels, drawn back
//!   as one premultiplied quad with the group's transform and opacity: the group's opacity is
//!   the group's, desktop·(1 − a) + group·a — its parts no longer show through each other.
//!
//! WHEN: a group run whenever a motion group of more than one layer fades (opacity < 255); the
//! below run when a layer is in motion and the slot's pass without it would draw more than the
//! executor's budget — then kept, unused at rest, while its layers stay the same list.
//! A want that finds no target is counted (`RunsShort`) and drawn as layers: nothing is refused.
//! A child of `plan`: it reads the plan's tables, nothing else does.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/frame_runs.rs` — a frame with the below run against the frame without
//!   over random presents, the group formula, `test_reject_a_run_drawn_after_a_change_inside_it`,
//!   `test_reject_a_run_holding_a_layer_that_moves_apart`, `test_reject_a_third_run`,
//!   `test_reject_a_run_as_a_layer_s_source`
//! INVARIANTS: a run is read only while it is what drawing its layers now would give — every
//!   plan is tracked, drawn or not (`Frame::plan` calls `track`): a full present, another
//!   wallpaper, a prefix that is not the same list or holds a layer in motion drop the below
//!   run; a layer drawn otherwise, a named rectangle that is not a higher layer's own and a
//!   glass written join its lack; a group is drawn again when its rest layers, a rectangle named
//!   in it or one of its glasses changed. Only the slot's pass reads a run, after the frame's
//!   glass writes and the runs' own passes. No allocation in a frame beyond a group's rest list,
//!   reserved when the run is taken. A run's pass cuts its layers around the opaque layers of
//!   the run alone: a glass back the layers' draw cuts around an opaque layer ABOVE the run is
//!   drawn whole in the run — the same texels but for a sampler that clamps at each piece
//!   (within a blur texel of that cut; `the_below_run_draws_a_glass_back_whole_...`).

use alloc::vec::Vec;

use super::{extent, same_drawing, Frame, LayerStep, Planned};
use crate::frame::board::{Board, PassKind, PassSpec, Target};
use crate::frame::chain::PassArea;
use crate::frame::damage::Area;
use crate::frame::glass::CacheStep;
use crate::frame::layers::{FrameLayer, RetainedLayers, MAX_LAYERS, ORIGIN_ABOVE};
use crate::frame::occlude::Px;
use crate::frame::reach::px;

/// The run targets an executor keeps (RFC-0101 §8): planes of the display's size.
pub const RUN_TARGETS: usize = 2;
/// Rows of a run's plane past the display that no pass ever draws: transparent texels, the
/// source a group's pass clears its rectangle from (a pass's own clear is not transparent).
pub(in crate::frame) const RUN_CLEAR_ROWS: u32 = 4;

/// The first of a run plane's transparent rows for a display of `display_rows`: past the
/// display on a boundary of 64 rows — a target written back in blocks (a tiled GPU's tiles)
/// has none that holds a display row and one of these.
pub(in crate::frame) const fn run_clear_row(display_rows: u32) -> u32 {
    display_rows.next_multiple_of(64)
}

/// The rows a run's plane spans for a display of `display_rows`: the display's and the
/// transparent ones. The executor makes its planes this tall, zeroed.
pub const fn run_plane_rows(display_rows: u32) -> u32 {
    run_clear_row(display_rows) + RUN_CLEAR_ROWS
}

/// What a run stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunKind {
    /// The wallpaper and the layers below the lowest one in motion.
    Below,
    /// The layers of this transform id, at rest.
    Group(u32),
}

/// A run as the frame's passes use it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunSpec {
    /// Its target: run plane `slot`.
    pub slot: u8,
    pub kind: RunKind,
    /// The plan's steps it stands for: `from..to`.
    pub from: usize,
    pub to: usize,
    /// What of its plane a pass draws again before the slot's pass reads it (inclusive
    /// corners); `None`: the plane holds the run.
    pub redraw: Option<PassArea>,
    /// Its rectangle in the plane: the display, or a group's bounds at rest.
    pub rect: Px,
    /// Where the slot's pass draws it, and at which opacity (`0..=255`).
    pub at: Px,
    pub opacity: u32,
}

/// What an executor gives the runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunLimits {
    /// Run planes it holds (`0..=RUN_TARGETS`).
    pub targets: usize,
    /// The textured pixels of a slot's pass above which the below run is taken.
    pub budget_px: u64,
    /// The draws one of its passes holds: a run whose own pass would ask more is not taken.
    pub draws: usize,
}

/// The wants of a frame that found no run: each is drawn as its layers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunsShort {
    /// The below run was due and every target was held, or its pass would not hold its draws,
    /// or a window lies across every place it could end.
    pub below: bool,
    /// Fading groups that found no target (or whose pass would not hold their draws).
    pub groups: u16,
    /// Fading groups that cannot be one run: their layers are not one range of the plan, or
    /// their rest bounds leave the display.
    pub apart: u16,
}

/// A run a target holds.
#[derive(Clone, Copy, Debug)]
struct Held {
    kind: RunKind,
    from: usize,
    to: usize,
    /// What changed the run's pixels since they were drawn.
    lack: Area,
    /// The wallpaper generation the below run was drawn over.
    epoch: u32,
}

/// The run targets' state across plans.
pub struct Runs {
    held: [Option<Held>; RUN_TARGETS],
    /// A group run's layers at rest, as they were drawn into its plane.
    rest: [Vec<FrameLayer>; RUN_TARGETS],
    /// What the frame's passes use: decided by `Frame::take_runs`, none after a plan.
    used: [Option<RunSpec>; RUN_TARGETS],
}

impl Runs {
    pub(super) const fn new() -> Self {
        Self {
            held: [None; RUN_TARGETS],
            rest: [Vec::new(), Vec::new()],
            used: [None; RUN_TARGETS],
        }
    }
}

/// The steps of transform id `id` when they are one range of the plan.
fn range_of(id: u32, steps: &[LayerStep]) -> Option<(usize, usize)> {
    let of = |s: &LayerStep| s.layer.layer_id == id;
    let from = steps.iter().position(of)?;
    let to = from + steps[from..].iter().take_while(|s| of(s)).count();
    (!steps[to..].iter().any(of)).then_some((from, to))
}

/// The highest place at or below `k` that no window lies across — no non-zero id has a step
/// below it and one at or above it; `0` when there is none above the wallpaper. The below run
/// ends there (R-own): a glass back is cut around a layer of its own window alone
/// (`pieces`), so the run's pass and the slot's make the same cuts of every back below it.
fn window_boundary(steps: &[LayerStep], mut k: usize) -> usize {
    loop {
        let (below, above) = steps.split_at(k.min(steps.len()));
        let first = |id: u32| below.iter().position(|s| id != 0 && s.layer.layer_id == id);
        match above.iter().filter_map(|s| first(s.layer.layer_id)).min() {
            Some(lower) => k = lower,
            None => return k,
        }
    }
}

/// Where a glass step's back changed this plan: written again, or drawn without rows.
fn glass_changed(s: &LayerStep) -> Area {
    let seen = Area::rect(s.layer.dst_x, s.layer.dst_y, s.layer.width, s.layer.height);
    match s.glass.map(|g| (g.cache, g.at)) {
        Some((CacheStep::Write(_), at)) => seen.union(Area::rect(at.0, at.1, at.2, at.3)),
        Some((CacheStep::Off, _)) => seen,
        _ => Area::None,
    }
}

/// The plan's tables a run is tracked against.
struct Tables<'a> {
    steps: &'a [LayerStep],
    prev: &'a [FrameLayer],
    carried: &'a [Option<usize>],
    named: &'a [(Area, u32)],
    layers: &'a RetainedLayers,
}

impl Tables<'_> {
    /// What this plan changed of the below run `[0, k)`; `None` when the prefix is no longer
    /// the same list — a layer of it came, went or swapped its place. A named rectangle is
    /// the prefix's unless its origin is above every layer or a layer at or above `k` whose
    /// window has no layer below `k` (`reach` R1: a change belongs to its origin's layer).
    fn below(&self, k: usize) -> Option<Area> {
        if self.steps.len() < k || self.prev.len() < k {
            return None;
        }
        let mut lack = Area::None;
        for (i, s) in self.steps.iter().enumerate().take(k) {
            if self.carried.get(i).copied().flatten() != Some(i) {
                return None;
            }
            if !same_drawing(&self.prev[i], &s.layer) {
                lack = lack.union(extent(&self.prev[i])).union(extent(&s.layer));
            }
            lack = lack.union(glass_changed(s));
        }
        let shared = |i: usize| {
            let id = self.steps.get(i).map_or(0, |s| s.layer.layer_id);
            id != 0 && self.steps[..k].iter().any(|s| s.layer.layer_id == id)
        };
        for &(area, origin) in self.named {
            let above = origin == ORIGIN_ABOVE
                || self.layers.index_of_key(origin).is_some_and(|i| i >= k && !shared(i));
            if !above {
                lack = lack.union(area);
            }
        }
        Some(lack)
    }

    /// Whether this plan changed the group run `from..to`: one of its glasses written or
    /// without rows, a rectangle named with one of its layers as origin, or one of unknown
    /// origin where the group is drawn.
    fn group(&self, (from, to): (usize, usize)) -> bool {
        let own = &self.steps[from..to];
        let drawn = own.iter().fold(Area::None, |a, s| a.union(extent(&s.layer)));
        own.iter().any(|s| glass_changed(s) != Area::None)
            || self.named.iter().any(|&(area, origin)| {
                origin != ORIGIN_ABOVE
                    && match self.layers.index_of_key(origin) {
                        Some(i) => (from..to).contains(&i),
                        None => area.touches(drawn),
                    }
            })
    }
}

impl Frame {
    /// A plan was made (`plan` calls this, whoever draws it): no run is used until the
    /// executor asks, and every held run takes what the plan changed of it, or is dropped.
    pub(super) fn track_runs(&mut self, fits: bool) {
        let Self { runs, steps, prev, carried, layers, glass, .. } = self;
        runs.used = [None; RUN_TARGETS];
        let (named, count, full) = layers.last_damage();
        let named = &named[..count.min(named.len())];
        let t = Tables { steps, prev, carried, named, layers };
        for held in runs.held.iter_mut() {
            let Some(run) = held else { continue };
            let kept = fits
                && !full
                && match run.kind {
                    RunKind::Below => {
                        let changed =
                            t.below(run.to).filter(|_| run.epoch == glass.wallpaper_epoch);
                        changed.map(|lack| run.lack = run.lack.union(lack)).is_some()
                    }
                    RunKind::Group(id) => match range_of(id, steps) {
                        Some(range) => {
                            if t.group(range) {
                                run.lack = Area::Full;
                            }
                            (run.from, run.to) = range;
                            true
                        }
                        None => false,
                    },
                };
            if !kept {
                *held = None;
            }
        }
    }

    /// The set and the runs forgotten: another compositor's layers are not these.
    pub fn forget(&mut self) {
        self.layers.clear();
        self.runs.held = [None; RUN_TARGETS];
        self.runs.used = [None; RUN_TARGETS];
    }

    /// Whether a layer of the plan is in motion: when an executor first needs its run planes.
    pub fn moving(&self) -> bool {
        self.steps.iter().any(|s| self.layers.in_motion(s.layer.layer_id))
    }

    /// The runs the frame's passes use, by target.
    pub fn runs(&self) -> [Option<RunSpec>; RUN_TARGETS] {
        self.runs.used
    }

    /// Decides the runs of the frame `planned` (the last plan), whose slot pass draws `area`
    /// of a `display` with `rows` (the layout's, the half scratch's): the executor draws what
    /// `runs` then names — a `Board` made afterwards lowers it — and may hold the targets from
    /// then on. `area` `None` (no pass this frame) decides nothing: the runs keep their lack.
    pub fn take_runs(
        &mut self,
        planned: Planned,
        (display, rows): ((u32, u32), (u32, u32)),
        area: Option<PassArea>,
        limits: RunLimits,
    ) -> RunsShort {
        let mut short = RunsShort::default();
        self.runs.used = [None; RUN_TARGETS];
        let n = self.steps.len().min(planned.layers).min(MAX_LAYERS);
        let Some(area) = area.filter(|_| n > 0) else { return short };
        let targets = limits.targets.min(RUN_TARGETS);
        let lowest = (0..n).find(|&i| self.layers.in_motion(self.steps[i].layer.layer_id));
        // What is held holds on: a below run under the lowest layer in motion, a group that
        // still fades as one — each in a target the executor still has.
        for slot in 0..RUN_TARGETS {
            let Some(run) = self.runs.held[slot] else { continue };
            let holds = slot < targets
                && match run.kind {
                    RunKind::Below => {
                        run.to <= n
                            && lowest.is_none_or(|k| run.to <= k)
                            && window_boundary(&self.steps[..n], run.to) == run.to
                    }
                    RunKind::Group(id) => self.fading(id, n, display).is_ok_and(|g| g.is_some()),
                };
            if !holds {
                self.runs.held[slot] = None;
            }
        }
        // The fading groups first (their look is the contract), lowest first.
        let mut i = 0;
        while i < n {
            let id = self.steps[i].layer.layer_id;
            let first = self.steps[..i].iter().all(|s| s.layer.layer_id != id);
            match self.fading(id, n, display) {
                Ok(Some(g)) => {
                    if !self.take_group(id, &g, targets) {
                        short.groups = short.groups.saturating_add(1);
                    }
                    i = g.to;
                }
                Err(()) if first => {
                    short.apart = short.apart.saturating_add(1);
                    i += 1;
                }
                _ => i += 1,
            }
        }
        // The below run: used while a layer moves; taken when the slot's pass is over budget.
        let Some(k) = lowest else { return short };
        let below = |h: &Option<Held>| h.is_some_and(|h| h.kind == RunKind::Below);
        let mut slot = self.runs.held.iter().position(below);
        if slot.is_none() && k > 0 {
            let over = {
                let b = Board::new(self, planned, display, rows.0, rows.1).with_area(area);
                b.pass(b.pass_count() - 1).map_or(0, |p| b.work_px(&p)) > limits.budget_px
            };
            // It ends on a window boundary (asked when it is due alone: a walk of the steps).
            let to = if over { window_boundary(&self.steps[..n], k) } else { 0 };
            slot = (0..targets).find(|&s| self.runs.held[s].is_none()).filter(|_| to > 0);
            short.below = over && slot.is_none();
            if let Some(s) = slot {
                let epoch = self.glass.wallpaper_epoch;
                self.runs.held[s] =
                    Some(Held { kind: RunKind::Below, from: 0, to, lack: Area::Full, epoch });
            }
        }
        if let Some((s, run)) = slot.and_then(|s| Some((s, self.runs.held[s].as_mut()?))) {
            let rect = (0, 0, display.0, display.1);
            self.runs.used[s] = Some(RunSpec {
                slot: s as u8,
                kind: RunKind::Below,
                from: 0,
                to: run.to,
                redraw: run.lack.on(display),
                rect,
                at: rect,
                opacity: 255,
            });
            run.lack = Area::None;
        }
        // A run's pass holds every draw of its range, or it is no run: an executor that draws
        // a pass's topmost `draws` alone would keep a plane short of its lowest layers.
        if self.runs.used.iter().flatten().any(|r| r.redraw.is_some()) {
            let mut over = [false; RUN_TARGETS];
            {
                let b = Board::new(self, planned, display, rows.0, rows.1).with_area(area);
                for spec in (0..b.pass_count()).filter_map(|p| b.pass(p)) {
                    if let Target::Run(s) = spec.target {
                        over[usize::from(s)] = b.draws(&spec) > limits.draws;
                    }
                }
            }
            for s in (0..RUN_TARGETS).filter(|&s| over[s]) {
                match self.runs.used[s].take().map(|r| r.kind) {
                    Some(RunKind::Below) => short.below = true,
                    Some(RunKind::Group(_)) => short.groups = short.groups.saturating_add(1),
                    None => {}
                }
                self.runs.held[s] = None;
            }
        }
        short
    }

    /// The motion group of transform id `id` among the plan's first `n` steps when it fades:
    /// `Ok(None)` when it does not fade or is one layer, `Err` when it fades and cannot be one
    /// run — its layers are not one range, or their bounds at rest leave the display.
    fn fading(&self, id: u32, n: usize, display: (u32, u32)) -> Result<Option<Group>, ()> {
        let Some(t) = self.layers.transform_of(id).filter(|t| t.opacity < 255) else {
            return Ok(None);
        };
        let (from, to) = range_of(id, &self.steps[..n]).ok_or(())?;
        if to - from < 2 {
            return Ok(None);
        }
        let rest = |j: usize| self.layers.rest(j).map_or(Area::None, |l| extent(&l));
        let bounds = (from..to).fold(Area::None, |a, j| a.union(rest(j)));
        let rect = px(bounds).filter(|_| bounds.inside(Area::rect(0, 0, display.0, display.1)));
        let (x, y, w, h) = rect.ok_or(())?;
        let mut q = FrameLayer { dst_x: x, dst_y: y, width: w, height: h, ..FrameLayer::EMPTY };
        q.opacity = 255;
        t.apply(&mut q);
        let at = (q.dst_x, q.dst_y, q.width, q.height);
        Ok(Some(Group { from, to, rect: (x, y, w, h), at, opacity: q.opacity }))
    }

    /// The group `g` of id `id` in a run: the one that holds it, or a free target; drawn again
    /// when its plane lacks anything or its layers at rest are not those it was drawn from.
    /// `false` when no target (or no room for its list) is left: it is drawn as layers.
    fn take_group(&mut self, id: u32, g: &Group, targets: usize) -> bool {
        let kind = RunKind::Group(id);
        let held = self.runs.held.iter().position(|h| h.is_some_and(|h| h.kind == kind));
        let free = (0..targets).find(|&s| self.runs.held[s].is_none());
        let Some(s) = held.or(free) else { return false };
        let Self { runs, layers, .. } = self;
        let now = || (g.from..g.to).filter_map(|j| layers.rest(j));
        let same = runs.held[s].is_some_and(|h| h.lack == Area::None)
            && runs.rest[s].len() == g.to - g.from
            && runs.rest[s].iter().zip(now()).all(|(a, b)| same_drawing(a, &b));
        if !same {
            runs.rest[s].clear();
            if runs.rest[s].try_reserve(g.to - g.from).is_err() {
                runs.held[s] = None;
                return false;
            }
            runs.rest[s].extend(now());
        }
        runs.held[s] = Some(Held { kind, from: g.from, to: g.to, lack: Area::None, epoch: 0 });
        let (x, y, w, h) = g.rect;
        runs.used[s] = Some(RunSpec {
            slot: s as u8,
            kind,
            from: g.from,
            to: g.to,
            redraw: (!same).then_some((x, y, x + w - 1, y + h - 1)),
            rect: g.rect,
            at: g.at,
            opacity: g.opacity,
        });
        true
    }
}

impl Board<'_> {
    /// R-own's cost in `pass`, counted (TASK-0351 P19): the pixels its glass backs draw under
    /// another window's opaque layer that a cut around that layer would have left out. A
    /// scene pass at the display's resolution (the slot's, a below run's); `0` for any other.
    pub fn uncut_px(&self, pass: &PassSpec) -> u64 {
        let PassKind::Scene { upto } = pass.kind else { return 0 };
        if self.reduction(pass).is_some() {
            return 0;
        }
        let (x0, y0, x1, y1) = pass.area;
        let clip = (x0, y0, (x1 + 1).saturating_sub(x0), (y1 + 1).saturating_sub(y0));
        let upto = upto.min(self.layers);
        (0..upto)
            .filter(|j| !self.in_run(pass, *j))
            .filter_map(|j| self.layer_draws(j, upto, false))
            .map(|d| d.uncut_px(clip))
            .sum()
    }
}

/// A fading group as one run: its steps, its bounds at rest, where and how it is drawn back.
struct Group {
    from: usize,
    to: usize,
    rect: Px,
    at: Px,
    opacity: u32,
}
