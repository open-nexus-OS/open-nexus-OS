// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A pass's draws walked once (TASK-0351 P10). `Board::draw(pass, i)` finds draw `i` from the
//! start — the wallpaper's occluder, then each layer's draws up to the one that holds `i` — so
//! an executor that asks per draw walks the plan's layers once per draw, and twice over when it
//! asks for the descriptors first (board cycle 83: 49 to 94 µs of CPU a draw, growing with the
//! windows). [`Draws`] keeps the walk's place instead: the wallpaper's pieces, then each layer's
//! draws from one `LayerDraws` built when the walk reaches the layer — the same draws in the
//! same order as the indexed walk (`frame_board_walk`), allocation-free. A write's scene pass
//! into the scratch yields each draw as that pass draws it: cut to the pixels its texels stand
//! for and at the blur's resolution (`chain::Reduced`, TASK-0351 P19).
//!
//! Runs (`plan::runs`, RFC-0109) are lowered here, in one place: the slot's pass draws a run's
//! quad where the walk reaches its first layer and goes on behind its last — the below run in
//! place of the wallpaper and its layers, opaque and one to one; a group run premultiplied,
//! where its transform puts it, at its opacity. Every other pass draws the layers (a write's
//! scene runs before the runs' own passes). A run's own pass: the below run's is a scene pass
//! up to its last layer; a group's clears its rectangle from the plane's transparent rows and
//! draws its layers at rest.

use super::board::{Board, DrawSpec, PassKind, PassSpec, Pipe, Target, Tex};
use super::chain::Reduced;
use super::occlude::{self, Px};
use super::pieces::LayerDraws;
use super::plan::{run_clear_row, RunKind, RunSpec, RUN_CLEAR_ROWS, RUN_TARGETS};
use crate::backend::gpu::quads::Quad;

impl Board<'_> {
    /// The runs whose planes a pass draws again this frame.
    fn redrawn(&self) -> impl Iterator<Item = &RunSpec> {
        self.runs.iter().flatten().filter(|r| r.redraw.is_some())
    }

    /// Passes that draw a run's plane again.
    pub(super) fn run_passes(&self) -> usize {
        self.redrawn().count()
    }

    /// The `n`-th of them: the below run's scene up to its last layer, a group's layers at rest.
    pub(super) fn run_pass(&self, n: usize) -> Option<PassSpec> {
        let r = self.redrawn().nth(n)?;
        let kind = match r.kind {
            RunKind::Below => PassKind::Scene { upto: r.to.min(self.layers) },
            RunKind::Group(_) => PassKind::Rest { from: r.from, to: r.to.min(self.layers) },
        };
        Some(PassSpec { target: Target::Run(r.slot), area: r.redraw?, kind })
    }

    /// The runs `pass` draws from: the slot's pass alone, a run it covers whole.
    fn runs_of(&self, pass: &PassSpec) -> [Option<RunSpec>; RUN_TARGETS] {
        match (pass.target, pass.kind) {
            (Target::Slot, PassKind::Scene { upto }) => {
                self.runs.map(|r| r.filter(|r| r.to <= upto))
            }
            _ => [None; RUN_TARGETS],
        }
    }

    /// Whether `pass` draws step `j` through a run and not as a layer.
    pub(super) fn in_run(&self, pass: &PassSpec, j: usize) -> bool {
        self.runs_of(pass).iter().flatten().any(|r| (r.from..r.to).contains(&j))
    }

    /// Whether `pass`'s draws are the walk's alone (a run's quads, a group's pass): the
    /// indexed walk knows the layers only.
    pub(super) fn by_walk(&self, pass: &PassSpec) -> bool {
        matches!(pass.kind, PassKind::Rest { .. }) || self.runs_of(pass).iter().any(Option::is_some)
    }

    /// Run `r` drawn back: the below run replaces, texel for pixel; a group is laid over the
    /// target premultiplied, every channel times its opacity.
    fn run_draw(r: &RunSpec) -> DrawSpec {
        let (x, row, w, h) = r.rect;
        let a = r.opacity.min(255) as f32 / 255.0;
        let (pipe, tint) = match r.kind {
            RunKind::Below => (Pipe::Opaque, [1.0; 4]),
            RunKind::Group(_) => (Pipe::Premultiplied, [a; 4]),
        };
        let quad =
            Quad::layer(r.at.0 as f32, r.at.1 as f32, r.at.2 as f32, r.at.3 as f32, tint, 0.0);
        DrawSpec { pipe, quad, tex: Tex::Run { slot: r.slot, x, row, w, h } }
    }

    /// A group pass's first draw: its area replaced by the transparent texels of the rows no
    /// pass draws (`run_clear_row`, past the display in its own plane).
    fn clear_draw(&self, pass: &PassSpec) -> Option<DrawSpec> {
        let Target::Run(slot) = pass.target else { return None };
        let (x0, y0, x1, y1) = pass.area;
        let (w, h) = ((x1 + 1).saturating_sub(x0), (y1 + 1).saturating_sub(y0));
        let n = RUN_CLEAR_ROWS;
        Some(DrawSpec {
            pipe: Pipe::Opaque,
            quad: Quad::layer(x0 as f32, y0 as f32, w as f32, h as f32, [1.0; 4], 0.0),
            tex: Tex::Run { slot, x: 0, row: run_clear_row(self.display.1), w: n, h: n },
        })
    }
}

/// The draws of one pass, in the order the indexed walk gives them.
pub struct Draws<'b, 'a> {
    board: &'b Board<'a>,
    pass: PassSpec,
    /// The scene pass's layers.
    upto: usize,
    /// The wallpaper's occluder and how many pieces it leaves.
    wall: (Option<Px>, usize),
    /// The next wallpaper piece, blur or single draw.
    i: usize,
    /// The layer the walk is at, its draws and the next of them.
    layer: usize,
    current: Option<(LayerDraws, usize)>,
    k: usize,
    /// A write's scene pass into the scratch: each draw placed at the blur's resolution.
    reduce: Option<Reduced>,
    /// The runs the pass draws from, each until the walk has drawn it.
    runs: [Option<RunSpec>; RUN_TARGETS],
}

impl<'b, 'a> Draws<'b, 'a> {
    pub(super) fn new(board: &'b Board<'a>, pass: &PassSpec) -> Self {
        let runs = board.runs_of(pass);
        let below = runs.iter().flatten().any(|r| r.kind == RunKind::Below);
        let (upto, wall, layer) = match pass.kind {
            // The below run holds the wallpaper: no piece of it is drawn.
            PassKind::Scene { upto } if below => (upto.min(board.layers), (None, 0), 0),
            PassKind::Scene { upto } => {
                let upto = upto.min(board.layers);
                let (w, h) = board.display;
                let occ = board.occluder((0, 0, w, h), 0, upto, false);
                (upto, (occ, occlude::count((0, 0, w, h), occ)), 0)
            }
            PassKind::Rest { from, to } => (to.min(board.layers), (None, 0), from),
            _ => (0, (None, 0), 0),
        };
        let reduce = board.reduction(pass);
        Self { board, pass: *pass, upto, wall, i: 0, layer, current: None, k: 0, reduce, runs }
    }

    /// The run whose first layer the walk stands at, taken: its quad, the walk behind it.
    fn run(&mut self) -> Option<DrawSpec> {
        let at = self.layer;
        let slot = self.runs.iter().position(|r| r.is_some_and(|r| r.from == at))?;
        let r = self.runs[slot].take()?;
        self.layer = r.to.max(at);
        Some(Board::run_draw(&r))
    }

    /// The next draw of the layers from the walk's place up to `upto`: as drawn, or at rest
    /// (a group run's pass).
    fn layers(&mut self, rest: bool) -> Option<DrawSpec> {
        while self.layer < self.upto {
            let (d, n) = match self.current {
                Some(c) => c,
                None => {
                    if let Some(run) = self.run() {
                        return Some(run);
                    }
                    match self.board.layer_draws(self.layer, self.upto, rest) {
                        Some(d) => {
                            let c = (d, d.count());
                            self.current = Some(c);
                            self.k = 0;
                            c
                        }
                        None => {
                            self.layer += 1;
                            continue;
                        }
                    }
                }
            };
            if self.k < n {
                let k = self.k;
                self.k += 1;
                return d.draw(k);
            }
            self.layer += 1;
            self.current = None;
        }
        None
    }

    fn scene(&mut self) -> Option<DrawSpec> {
        let (occ, wall) = self.wall;
        if self.i < wall {
            let (w, h) = self.board.display;
            let p = occlude::piece((0, 0, w, h), occ, self.i)?;
            self.i += 1;
            return Some(self.board.wallpaper_piece(self.upto, p));
        }
        self.layers(false)
    }
}

impl Iterator for Draws<'_, '_> {
    type Item = DrawSpec;

    fn next(&mut self) -> Option<DrawSpec> {
        match self.pass.kind {
            PassKind::One(d) => {
                let first = self.i == 0;
                self.i += 1;
                first.then_some(d)
            }
            PassKind::Blurs { .. } => {
                let i = self.i;
                self.i += 1;
                self.board.draw(&self.pass, i)
            }
            PassKind::Scene { .. } => {
                let d = self.scene()?;
                Some(self.reduce.map_or(d, |r| r.place(&d, self.pass.area)))
            }
            PassKind::Rest { .. } => {
                let first = self.i == 0;
                self.i += 1;
                match first.then(|| self.board.clear_draw(&self.pass)).flatten() {
                    Some(clear) => Some(clear),
                    None => self.layers(true),
                }
            }
        }
    }
}
