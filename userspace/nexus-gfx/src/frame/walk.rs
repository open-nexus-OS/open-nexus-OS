// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A pass's draws walked once (TASK-0351 P10). `Board::draw(pass, i)` finds draw `i` from the
//! start — the wallpaper's occluder, then each layer's draws up to the one that holds `i` — so
//! an executor that asks per draw walks the plan's layers once per draw, and twice over when it
//! asks for the descriptors first (board cycle 83: 49 to 94 µs of CPU a draw, growing with the
//! windows). [`Draws`] keeps the walk's place instead: the wallpaper's pieces, then each layer's
//! draws from one `LayerDraws` built when the walk reaches the layer — the same draws in the
//! same order as the indexed walk (`frame_board_walk`), allocation-free.

use super::board::{Board, DrawSpec, PassKind, PassSpec};
use super::occlude::{self, Px};
use super::pieces::LayerDraws;

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
}

impl<'b, 'a> Draws<'b, 'a> {
    pub(super) fn new(board: &'b Board<'a>, pass: &PassSpec) -> Self {
        let (upto, wall) = match pass.kind {
            PassKind::Scene { upto } => {
                let upto = upto.min(board.layers);
                let (w, h) = board.display;
                let occ = board.occluder((0, 0, w, h), 0, upto);
                (upto, (occ, occlude::count((0, 0, w, h), occ)))
            }
            _ => (0, (None, 0)),
        };
        Self { board, pass: *pass, upto, wall, i: 0, layer: 0, current: None, k: 0 }
    }

    fn scene(&mut self) -> Option<DrawSpec> {
        let (occ, wall) = self.wall;
        if self.i < wall {
            let (w, h) = self.board.display;
            let p = occlude::piece((0, 0, w, h), occ, self.i)?;
            self.i += 1;
            return Some(self.board.wallpaper_piece(self.upto, p));
        }
        while self.layer < self.upto {
            let (d, n) = match self.current {
                Some(c) => c,
                None => match self.board.layer_draws(self.layer, self.upto) {
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
                },
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
                (i < self.board.write_count).then(|| self.board.draw(&self.pass, i)).flatten()
            }
            PassKind::Scene { .. } => self.scene(),
        }
    }
}
