// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The kit widgets' CONTINUOUS loops (`AnimKind::Loop`): the sweeps (the indeterminate pip, the
//! skeleton's shimmer band) cross their track from outside it under its rounded clip, the
//! spinner's carousel steps its spokes, the level bars rise and fall staggered, the skeleton
//! breathes. Positions are functions of the clock (`loop_math`); the breathe is a spring that
//! re-fires. Split out of `probe/anim.rs` (TASK-0354 P13, the structure ratchet).

use super::*;
use crate::loop_math::{level_scale_pct, sweep_of};
use nexus_dsl_runtime::LOOP_LEVEL;
use nexus_scene_raster::RoundClip;

/// The bars of a `LevelBars` row (its children `1..=4`, `nexus_dsl_runtime`'s builder).
const LEVEL_BARS: usize = 4;

/// Continuous-loop (`AnimKind::Loop`) breathe opacity endpoints + midpoint —
/// an inherently-animated widget (Skeleton) pulses between these forever via a
/// spring that re-fires toward the opposite endpoint on each convergence
/// (alloc-free: `spring_to` allocates nothing, so the loop never grows the heap).
const BREATHE_BRIGHT: f32 = 1.0;
const BREATHE_DIM: f32 = 0.15;
const BREATHE_MID: f32 = 0.55;

/// Carousel step interval (`LOOP_CAROUSEL`): one revolution in exactly 1 s over the 12 spokes
/// (the design artifacts' activity ring, 83.3 ms a spoke).
const SPINNER_STEP_NS: u64 = 1_000_000_000 / LOOP_CAROUSEL_SPOKES as u64;

/// One continuous widget loop (reconciled against the emitted Loop intents).
#[derive(Clone, Copy)]
pub(super) struct LoopEnt {
    /// The widget ROOT node the intent targets.
    node_id: usize,
    /// Loop sub-kind (`nexus_dsl_runtime::LOOP_*`, stamped in the intent value).
    sub: i32,
    /// Carousel: current leading-spoke step on the time grid.
    step: u8,
}

impl AnimState {
    /// A slow, smooth (over-damped) breathe spring — a calm ~1s approach with
    /// no overshoot, so the loop reads as a gentle pulse, not a bounce.
    fn breathe_config() -> SpringConfig {
        SpringConfig { stiffness: 18.0, damping: 9.0, mass: 1.0, initial_velocity: 0.0 }
    }

    /// (Re)start a breathe half-cycle for `node_id`: spring the opacity from its
    /// current value toward the OPPOSITE endpoint (dim when currently bright,
    /// bright when currently dim). Alloc-free (`spring_to` carries no Vec).
    fn start_breathe(&mut self, node_id: usize) {
        let cur = self.cur(node_id, AnimProp::Opacity);
        let target = if cur > BREATHE_MID { BREATHE_DIM } else { BREATHE_BRIGHT };
        self.driver.spring_to(
            LayerId(node_id as u64),
            AnimProp::Opacity,
            cur,
            target,
            Self::breathe_config(),
        );
    }

    /// Write a sweep's place at `now_ns` (the pip's or the shimmer band's, on its time grid)
    /// into the part's transform; whether it moved. The part crosses its track FROM OUTSIDE it,
    /// cut by the track's rounded clip (design artifacts: PRG-5); a track inside a scroll layer
    /// (whose band paints in its own space) keeps the inside travel and no clip. Under a
    /// reduced-motion theme the part rests outside the track (hidden). Alloc-free.
    fn write_sweep(
        &mut self,
        ent: LoopEnt,
        now_ns: u64,
        boxes: &[nexus_layout::LayoutBox],
        reduced: bool,
    ) -> bool {
        let Some(sweep) = sweep_of(ent.sub) else { return false };
        let find = |id: usize| boxes.iter().find(|b| b.node_id == id);
        let (Some(track), Some(part)) = (find(ent.node_id), find(ent.node_id + 1)) else {
            return false;
        };
        let (tw, pw) = (track.rect.width.0, part.rect.width.0);
        let inset = part.rect.x.0 - track.rect.x.0;
        let (x, clip) = if track.scroll_root.is_none() {
            let at = if reduced { -pw } else { sweep.from_outside(now_ns, tw, pw) };
            let r = track.visual.corner_radius.top_left.0;
            let clip = RoundClip {
                x: track.rect.x.0,
                y: track.rect.y.0,
                w: tw,
                h: track.rect.height.0,
                radius: r,
            };
            (at - inset, Some(clip))
        } else {
            (sweep.at(now_ns, (tw - pw).max(0)), None)
        };
        let slot = self.node_mut(ent.node_id + 1);
        let moved = slot.dx != x || slot.clip != clip;
        slot.dx = x;
        slot.clip = clip;
        moved
    }

    /// Write the level bars' scales at `now_ns` (bar `k` is child `k` of the row), scaled from
    /// their bottom (`expand_node_anims` anchors them there, [`Self::level_bar`]); at rest (100 %)
    /// under a reduced-motion theme. Whether any moved. Alloc-free.
    fn write_level(&mut self, ent: LoopEnt, now_ns: u64, reduced: bool) -> bool {
        let mut moved = false;
        for k in 1..=LEVEL_BARS {
            let pct = if reduced { 100 } else { level_scale_pct(now_ns, k as u64 - 1) };
            let want = (pct != 100).then_some(pct);
            let slot = self.node_mut(ent.node_id + k);
            if slot.scale_y_pct != want {
                slot.scale_y_pct = want;
                moved = true;
            }
        }
        moved
    }

    /// Whether `node_id` is one of a live `LevelBars` row's bars (scaled from its bottom).
    pub(super) fn level_bar(&self, node_id: usize) -> bool {
        self.loops.iter().any(|e| {
            e.sub == LOOP_LEVEL && node_id > e.node_id && node_id <= e.node_id + LEVEL_BARS
        })
    }

    /// Write the carousel's stepped spoke opacities: the leading spoke is opaque, the one `d`
    /// behind it `(n − d)/n` — the Spinner builder's resting `(i+1)/12` fade, rotated.
    fn write_carousel(&mut self, node_id: usize, step: u8) {
        let n = LOOP_CAROUSEL_SPOKES;
        for i in 0..n {
            let d = (i + n - step as usize) % n;
            let a = (255 * (n - d) / n) as u8;
            self.node_mut(node_id + 1 + i).opacity = a;
        }
    }

    /// Forget one loop's paint state (the widget left the tree): cancel its
    /// springs and reset the touched nodes to identity so nothing stays faded.
    fn forget_loop(&mut self, ent: LoopEnt) {
        match ent.sub {
            LOOP_SWEEP | LOOP_SHIMMER => {
                let child = ent.node_id + 1;
                self.set_prop(child, AnimProp::TranslateX, 0.0);
                self.node_mut(child).clip = None;
                self.prune_identity(child);
            }
            LOOP_LEVEL => {
                for k in 1..=LEVEL_BARS {
                    self.node_mut(ent.node_id + k).scale_y_pct = None;
                    self.prune_identity(ent.node_id + k);
                }
            }
            LOOP_CAROUSEL => {
                for i in 0..LOOP_CAROUSEL_SPOKES {
                    let c = ent.node_id + 1 + i;
                    self.node_mut(c).opacity = 255;
                    self.prune_identity(c);
                }
            }
            _ => {
                self.driver.cancel(LayerId(ent.node_id as u64), AnimProp::Opacity);
                self.set_prop(ent.node_id, AnimProp::Opacity, BREATHE_BRIGHT);
                self.prune_identity(ent.node_id);
            }
        }
    }

    /// Reconcile the continuous-loop set with the freshly-emitted `Loop`
    /// intents: forget loops gone from the tree and start each newly-seen
    /// looping widget's motion (sub-kind from the intent value).
    pub(super) fn sync_loops(&mut self, present: &[(usize, AnimIntent)]) {
        let is_present = |e: &LoopEnt| {
            present
                .iter()
                .any(|(pn, pi)| *pn == e.node_id && pi.kind == AnimKind::Loop && pi.value == e.sub)
        };
        let mut i = 0;
        while i < self.loops.len() {
            if is_present(&self.loops[i]) {
                i += 1;
            } else {
                let ent = self.loops.swap_remove(i);
                self.forget_loop(ent);
            }
        }
        // Start newly-seen loops.
        for &(node_id, intent) in present {
            if intent.kind != AnimKind::Loop || self.loops.iter().any(|e| e.node_id == node_id) {
                continue;
            }
            if self.loops.len() >= MAX_NODE_ANIMS {
                break;
            }
            // step 0xFF = "not painted yet": the first tick always writes.
            self.loops.push(LoopEnt { node_id, sub: intent.value, step: 0xFF });
            match intent.value {
                // The sweeps and the carousel paint on the first tick's time grid.
                LOOP_SWEEP | LOOP_SHIMMER | LOOP_CAROUSEL | LOOP_LEVEL => {}
                _ => self.start_breathe(node_id),
            }
        }
    }

    /// Keep every loop alive: a sweep's part moves to its place on the time grid, the carousel
    /// advances its leading spoke on its own, and a converged breathe springs to the opposite
    /// endpoint. Returns the union ROW SPAN of loop-driven paint changes the driver's own
    /// updates do NOT cover (the sweeps and the carousel steps). Alloc-free.
    pub(super) fn tick_loops(
        &mut self,
        now_ns: u64,
        boxes: &[nexus_layout::LayoutBox],
        reduced: bool,
    ) -> Option<(i32, i32)> {
        let mut span: Option<(i32, i32)> = None;
        let grow =
            |boxes: &[nexus_layout::LayoutBox], node_id: usize, span: &mut Option<(i32, i32)>| {
                if let Some(b) = boxes.iter().find(|b| b.node_id == node_id) {
                    let (y0, y1) = (b.rect.y.0 - 1, b.rect.y.0 + b.rect.height.0 + 1);
                    *span = Some(match *span {
                        Some((s0, s1)) => (s0.min(y0), s1.max(y1)),
                        None => (y0, y1),
                    });
                }
            };
        for idx in 0..self.loops.len() {
            let ent = self.loops[idx];
            match ent.sub {
                LOOP_SWEEP | LOOP_SHIMMER => {
                    if self.write_sweep(ent, now_ns, boxes, reduced) {
                        grow(boxes, ent.node_id, &mut span);
                    }
                }
                LOOP_LEVEL => {
                    if self.write_level(ent, now_ns, reduced) {
                        grow(boxes, ent.node_id, &mut span);
                    }
                }
                LOOP_CAROUSEL => {
                    let step = ((now_ns / SPINNER_STEP_NS) % LOOP_CAROUSEL_SPOKES as u64) as u8;
                    if step != ent.step {
                        self.loops[idx].step = step;
                        self.write_carousel(ent.node_id, step);
                        grow(boxes, ent.node_id, &mut span);
                    }
                }
                _ => {
                    if !self.driver.is_active(LayerId(ent.node_id as u64), AnimProp::Opacity) {
                        self.start_breathe(ent.node_id);
                    }
                }
            }
        }
        span
    }
}

/// Whether `node_id` is a part one of `loops` paints (kept across re-emits: a loop part carries
/// no driver track at every instant, and dropping it mid-cycle would flash it to identity). A
/// free function over the loop list so the caller can hold the transform list mutably beside it.
pub(super) fn is_loop_part(loops: &[LoopEnt], node_id: usize) -> bool {
    loops.iter().any(|e| match e.sub {
        LOOP_SWEEP | LOOP_SHIMMER => node_id == e.node_id + 1,
        LOOP_CAROUSEL => node_id > e.node_id && node_id <= e.node_id + LOOP_CAROUSEL_SPOKES,
        LOOP_LEVEL => node_id > e.node_id && node_id <= e.node_id + LEVEL_BARS,
        _ => node_id == e.node_id,
    })
}
