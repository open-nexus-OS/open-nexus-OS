// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The store-reuse invariant, end to end on the host (package B 1a): a motion layer's store rows
//! equal a fresh paint of its subtree at rest after any sequence of edits inside it, renumbers,
//! exits, revives, drops (parked), re-enters, other items coming and going over the store
//! (painting their places), compactions and theme changes. The world below mirrors the probe's
//! glue (`probe/motion_layers/reuse.rs`, `probe/store.rs`, `leave.rs`) over the real decision
//! (`reuse`, `Cache`, `give_way`), the real allocator (`store_alloc`), the real diff
//! (`layout_diff`) and the real row painter (`pass_plan`). 200 seeded sequences, the pattern of
//! TASK-0367 P3's 200-edit oracle (`pass_plan_diff_tests.rs`).

use super::tests::{
    build, edit_panel, layer_rect, outside, paint_rows, panel, size, Doc, Node, Rng, H, IDENT,
    LAYER_KEY, PARKED_KEY, W,
};
use super::*;
use crate::store_alloc::{Item, StoreAlloc};
use std::vec::Vec;

/// The store's columns and rows (the allocator's place).
const SW: u32 = 320;
const SH: u32 = 520;
/// Another item of the store (a band, another layer) that paints its place.
const NOISE_KEY: u32 = 1 << 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Life {
    Live,
    Ghost,
}

#[derive(Clone, Copy, Debug)]
struct Layer {
    id: u16,
    span: (usize, usize),
    rect: Span,
    place: Option<(u16, u16)>,
    painted: bool,
    life: Life,
}

/// What the sequences did: decisions by kind, parked stores taken over.
#[derive(Default, Debug)]
struct Counts {
    keep: usize,
    rows: usize,
    whole: usize,
    adopted: usize,
    checked: usize,
}

/// One surface: the layout, the store's pixels and places, the cache, slot 0's layer.
struct World {
    rng: Rng,
    prefix: Vec<Node>,
    panel: Vec<Node>,
    doc: Doc,
    span: (usize, usize),
    theme: u8,
    alloc: StoreAlloc,
    store: Vec<u8>,
    cache: Cache,
    layer: Option<Layer>,
    noise: Option<(u32, u32)>,
    ids: u16,
    rows: Option<(i32, i32)>,
    counts: Counts,
}

fn union(rows: &mut Option<(i32, i32)>, (y0, y1): (i32, i32)) {
    *rows = Some(rows.map_or((y0, y1), |s| (s.0.min(y0), s.1.max(y1))));
}

impl World {
    fn new(seed: u64) -> Self {
        let mut rng = Rng(seed);
        let prefix = vec![outside(&mut rng), outside(&mut rng)];
        let panel = panel(&mut rng);
        let (doc, span) = build(&prefix, &panel);
        Self {
            rng,
            prefix,
            panel,
            doc,
            span,
            theme: 0,
            alloc: StoreAlloc::new(SW, 0, SH),
            store: vec![0; (SW * SH * 4) as usize],
            cache: Cache::new(),
            layer: None,
            noise: None,
            ids: 0,
            rows: None,
            counts: Counts::default(),
        }
    }

    fn rebuild(&mut self) -> Doc {
        let (doc, span) = build(&self.prefix, &self.panel);
        self.span = span;
        core::mem::replace(&mut self.doc, doc)
    }

    /// `take_now` (probe): what the live, placed layer's store would hold, into the scratch.
    fn take_now(&mut self) -> bool {
        let Some(l) = self.layer.filter(|l| l.life == Life::Live) else { return false };
        let Some(place) = l.place else { return false };
        let lists = (self.doc.boxes.as_slice(), self.doc.texts.as_slice());
        self.cache.now_mut().take(lists, l.span, (l.rect, place), Inputs::new(self.theme, None));
        true
    }

    /// `reuse_store` (probe).
    fn reuse_store(&mut self) {
        let r = if self.take_now() { self.cache.decide(0) } else { Reuse::Whole };
        if r == Reuse::Whole {
            self.cache.forget(0);
        }
        match r {
            Reuse::Keep => self.counts.keep += 1,
            Reuse::Rows(_) => self.counts.rows += 1,
            Reuse::Whole => self.counts.whole += 1,
        }
        if let Some(l) = self.layer.as_mut().filter(|l| l.life == Life::Live) {
            l.painted = r != Reuse::Whole;
        }
        if let Reuse::Rows(rows) = r {
            union(&mut self.rows, rows);
        }
    }

    /// `plan_store` + `place_motions` (probe): the items, the order of giving way, the places.
    fn plan(&mut self) {
        self.cache.drop_parked_off_theme(self.theme);
        loop {
            let mut items = Vec::new();
            if let Some(l) = self.layer {
                let (w, h) = size(l.rect);
                items.push(Item { key: LAYER_KEY | u32::from(l.id), w, h });
            }
            if let Some((w, h)) = self.noise {
                items.push(Item { key: NOISE_KEY, w, h });
            }
            let mut np = 0;
            self.cache.parked_items(|key, w, h| {
                items.push(Item { key, w, h });
                np += 1;
            });
            let outcome = self.alloc.plan(&items);
            let ghost = self.layer.is_some_and(|l| l.life == Life::Ghost);
            let live = self.layer.is_some_and(|l| l.life == Life::Live);
            match give_way(outcome, (np > 0, ghost, live)) {
                GiveWay::Placed => break,
                GiveWay::Parked => {
                    self.cache.drop_parked();
                }
                GiveWay::Ghosts(_) => {
                    self.cache.drop_parked();
                    self.layer = None;
                    self.cache.forget(0);
                }
                GiveWay::Motions => {
                    // No room: the app paints it in the base — out of this model.
                    self.layer = None;
                    self.cache.forget(0);
                }
                GiveWay::Band => {
                    if self.noise.take().is_none() {
                        let _ = self.alloc.plan(&[]);
                        break;
                    }
                }
            }
        }
        // Another item paints its place: whatever lay there is gone.
        if let Some((x, y, w, h)) = self.alloc.place_of(NOISE_KEY) {
            let v = self.rng.next() as u8 | 1;
            for row in y..y + h {
                let at = ((row * SW + x) * 4) as usize;
                self.store[at..at + (w * 4) as usize].fill(v);
            }
        }
        let Some(l) = self.layer else { return };
        let place = self
            .alloc
            .place_of(LAYER_KEY | u32::from(l.id))
            .map(|(x, y, _, _)| (x as u16, y.min(0xFFFF) as u16));
        if place == l.place {
            return;
        }
        if l.life == Life::Ghost {
            self.layer = None;
            self.cache.forget(0);
            return;
        }
        self.layer = Some(Layer { place, ..l });
        self.reuse_store();
    }

    /// Where store row `y` (layout row) of layer `l` at `(sx, sy)` starts in the store's bytes.
    fn at(l: &Layer, (sx, sy): (u16, u16), y: i32) -> usize {
        (((u32::from(sy) + (y - l.rect.1) as u32) * SW + u32::from(sx)) * 4) as usize
    }

    /// Store rows `rows` of the layer painted from the layout now (`redraw_motion`).
    fn redraw(&mut self, rows: (i32, i32)) {
        let Some(l) = self.layer else { return };
        let Some(place) = l.place else { return };
        let (y0, y1) = (rows.0.max(l.rect.1), rows.1.min(l.rect.3));
        if y1 <= y0 {
            return;
        }
        self.cache.forget(0);
        for (y, px) in paint_rows(&self.doc, l.span, l.rect, (y0, y1)) {
            let at = Self::at(&l, place, y);
            self.store[at..at + px.len()].copy_from_slice(&px);
        }
    }

    /// A present (`band.rs` → `redraw_motion_rows`, `paint_motion_layers`, `hold_painted`).
    fn present(&mut self) {
        let Some(l) = self.layer.filter(|l| l.life == Life::Live && l.place.is_some()) else {
            self.rows = None;
            return;
        };
        if let Some(rows) = self.rows.take().filter(|_| l.painted) {
            self.redraw(rows);
        }
        if !l.painted {
            self.redraw((l.rect.1, l.rect.3));
            if let Some(m) = self.layer.as_mut() {
                m.painted = true;
            }
        }
        if self.cache.wants(0) && self.take_now() {
            self.cache.hold_now(0);
        }
    }

    /// The invariant: a placed, painted, live layer's store rows are a fresh paint.
    fn check(&mut self, what: &str) {
        let Some(l) = self.layer.filter(|l| l.life == Life::Live && l.painted) else { return };
        let Some(place) = l.place else { return };
        self.counts.checked += 1;
        for (y, px) in paint_rows(&self.doc, l.span, l.rect, (l.rect.1, l.rect.3)) {
            let at = Self::at(&l, place, y);
            let held = &self.store[at..at + px.len()];
            if let Some(x) = held.iter().zip(&px).position(|(a, b)| a != b) {
                panic!("{what}: store pixel ({},{y}) of the layer is not a fresh paint", x / 4);
            }
        }
    }

    /// After a change of the layout: what the layer holds is routed to its rows (P5,
    /// `layout_change` with the layer's numbering before the re-resolve), then the layer
    /// follows its identity (`sync_motion_layers`).
    fn relayout(&mut self, old: &Doc) {
        let Some(l) = self.layer.filter(|l| l.life == Life::Live) else { return };
        let held = |id: usize| id >= l.span.0 && id < l.span.1;
        let texts = (old.texts.as_slice(), self.doc.texts.as_slice());
        let changed = crate::layout_diff::changed_rows(
            &old.boxes,
            &self.doc.boxes,
            texts,
            (W, H),
            (&held, &held),
        );
        if let Some(rows) = changed.layers {
            union(&mut self.rows, rows);
        }
        let Some(rect) = layer_rect(&self.doc, self.span) else { return };
        if (rect, self.span) != (l.rect, l.span) {
            let resized = size(rect) != size(l.rect);
            self.layer = Some(Layer { rect, span: self.span, ..l });
            self.reuse_store();
            if resized {
                self.plan();
            }
        }
    }

    /// `drop_due_ghosts` → `park_store`.
    fn drop_ghost(&mut self, l: Layer) {
        self.layer = None;
        if let Some(place) = l.place {
            let alloc = &mut self.alloc;
            self.cache.park(0, (IDENT, size(l.rect), place), PARKED_KEY, |(x, y), key| {
                alloc.rekey_at((u32::from(x), u32::from(y)), key)
            });
        }
        self.cache.forget(0);
        self.plan();
    }

    /// `start_motion_layer` → `adopt_parked`, then the plan places it.
    fn enter(&mut self, rect: Span) {
        self.ids = self.ids.wrapping_add(1).max(1);
        let id = self.ids;
        self.cache.forget(0);
        if let Some(j) = parked_for(self.cache.parked(), IDENT, size(rect)) {
            if let Some((x, y)) = self.cache.unpark(j, 0) {
                let key = LAYER_KEY | u32::from(id);
                if self.alloc.rekey_at((u32::from(x), u32::from(y)), key) {
                    self.counts.adopted += 1;
                } else {
                    self.cache.forget(0);
                }
            }
        }
        let life = Life::Live;
        self.layer = Some(Layer { id, span: self.span, rect, place: None, painted: false, life });
        self.plan();
    }

    fn step(&mut self) -> &'static str {
        match self.rng.below(12) {
            0..=2 => {
                let did = edit_panel(&mut self.panel, &mut self.rng);
                let old = self.rebuild();
                self.relayout(&old);
                did
            }
            3 => {
                if self.prefix.len() > 1 && self.rng.below(2) == 0 {
                    self.prefix.pop();
                } else {
                    let n = outside(&mut self.rng);
                    self.prefix.push(n);
                }
                let old = self.rebuild();
                self.relayout(&old);
                "renumber"
            }
            4 | 5 => match self.layer {
                // `leave`: a drawn layer leaves as a ghost.
                Some(l) if l.life == Life::Live && l.painted && l.place.is_some() => {
                    self.layer = Some(Layer { life: Life::Ghost, ..l });
                    "exit"
                }
                _ => "exit (nothing drawn)",
            },
            6 => match self.layer {
                // `revive` + `revive_as`.
                Some(l) if l.life == Life::Ghost => {
                    let Some(rect) = layer_rect(&self.doc, self.span) else {
                        return "revive (empty)";
                    };
                    let grew = size(rect) != size(l.rect);
                    self.layer = Some(Layer { rect, span: self.span, life: Life::Live, ..l });
                    self.reuse_store();
                    if grew {
                        self.plan();
                    }
                    "revive"
                }
                _ => "revive (no ghost)",
            },
            7 => match self.layer {
                Some(l) if l.life == Life::Ghost => {
                    self.drop_ghost(l);
                    "drop"
                }
                _ => "drop (no ghost)",
            },
            8 | 9 => {
                if self.layer.is_some() {
                    return "enter (slot taken)";
                }
                let Some(rect) = layer_rect(&self.doc, self.span) else { return "enter (empty)" };
                self.enter(rect);
                "enter"
            }
            10 => {
                self.noise = match self.noise {
                    Some(_) if self.rng.below(2) == 0 => None,
                    _ => Some((10 + self.rng.below(310) as u32, 10 + self.rng.below(300) as u32)),
                };
                self.plan();
                "another item comes or goes"
            }
            _ => {
                // A theme change: the base and every live layer paint whole (`Change::Whole`);
                // a ghost's or a parked store's snapshot is of the old theme.
                self.theme ^= 1;
                if self.layer.is_some_and(|l| l.life == Life::Live && l.place.is_some()) {
                    if let Some(l) = self.layer.as_mut() {
                        l.painted = false;
                    }
                    self.cache.forget(0);
                }
                self.plan();
                "theme"
            }
        }
    }
}

/// ≥ 200 random sequences: after every step, a placed, painted layer's store rows equal a fresh
/// paint of its subtree at rest, byte for byte.
#[test]
fn a_store_holds_a_fresh_paint_after_any_sequence() {
    let mut total = Counts::default();
    for s in 0..200u64 {
        let mut world =
            World::new(0x9E37_79B9_7F4A_7C15 ^ (s.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1));
        let mut log = Vec::new();
        for n in 0..20 {
            let did = world.step();
            log.push(did);
            world.present();
            world.check(&std::format!("sequence {s} step {n} ({did}) after {log:?}"));
        }
        total.keep += world.counts.keep;
        total.rows += world.counts.rows;
        total.whole += world.counts.whole;
        total.adopted += world.counts.adopted;
        total.checked += world.counts.checked;
    }
    // The sequences exercise every branch, and the stores were checked often (seeded: the
    // counts are the same on every run; the bounds sit well under them).
    assert!(total.keep >= 150, "{total:?}");
    assert!(total.rows >= 80, "{total:?}");
    assert!(total.whole >= 150, "{total:?}");
    assert!(total.adopted >= 20, "{total:?}");
    assert!(total.checked >= 1200, "{total:?}");
}
