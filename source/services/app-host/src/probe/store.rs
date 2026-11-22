// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host's STORE plan (RFC-0109): what the surface's rows below its base hold — the
//! wash patch, each scroll layer's virtual band, each motion layer's pixels (TASK-0354 P2) —
//! placed on shelves, and what a create asks windowd for. Split out of `band.rs` (module-size
//! ratchet): the bands' filling and drawing stay there, the motion layers' raster is
//! `motion_layers.rs`. Arithmetic only: a plan that outgrows the surface's rows waits for the
//! loop's re-create.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the bounds in `nexus-display-proto::surface_layers`; the QEMU lanes.

use super::band::{Band, PATCH_ROWS};
use super::motion_layers::MAX_MOTION_LAYERS;
use super::*;
use crate::motion_reuse::{give_way, parked_for, GiveWay, MAX_PARKED};
use crate::store_alloc::{Item, Outcome, StoreAlloc, MAX_ITEMS};
use nexus_display_proto::surface_layers as lw;

/// The least margin a band keeps beyond its viewport (content positions).
const MARGIN_MIN: u32 = 64;
/// The patch row's columns: the hover wash's patch and one per motion layer's backdrop.
const PATCH_COLS: u32 = PATCH_ROWS * lw::MAX_PATCHES as u32;

/// What a surface's store takes (RFC-0109): the surface's rows (base + store) and the store's
/// used width — the region windowd places for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StorePlan {
    pub(super) rows: u32,
    pub(super) width: u16,
}

/// Where each motion layer's pixels go in the store (`None`: not placed).
type Places = [Option<(u16, u16)>; MAX_MOTION_LAYERS];

/// A band's key in the store's places (`store_alloc`): its scroll layer's index.
const BAND_KEY: u32 = 1 << 16;
/// A motion layer's key: its id (the layer's life — a leaving layer keeps its place).
const LAYER_KEY: u32 = 1 << 17;
/// A parked store's key (`crate::motion_reuse`): its parking's sequence number.
const PARKED_KEY: u32 = 1 << 19;
const _: () = {
    /// The ghost bands the bound holds room for beside every live item and parked store (past
    /// it the parked stores go, then the ghosts: `bound`).
    const GHOST_BANDS: usize = 4;
    assert!(
        super::scroll::MAX_SCROLL_LAYERS + MAX_MOTION_LAYERS + GHOST_BANDS + MAX_PARKED
            <= MAX_ITEMS
    );
};

/// Scroll layer `l`'s band with `margin_pct` of its viewport as margin each way, in a store `w`
/// wide: `(strip_len, strips, width, height)` — vertical: one strip of the ring's rows, the
/// viewport wide; horizontal: strips of `strip_len` columns (no wider than the surface) under
/// each other. `None`: no band of it fits that store.
fn band_shape(
    l: &super::scroll::ScrollLayer,
    margin_pct: u32,
    w: u32,
) -> Option<(u32, u32, u32, u32)> {
    let (vx0, _, vx1, _) = l.viewport;
    let (vw, vh) = ((vx1 - vx0).max(1) as u32, l.band_rows().max(1) as u32);
    let len = l.viewport_len().max(1) as u32;
    let margin = (len * margin_pct / 100).max(MARGIN_MIN);
    let cap = len + 2 * margin;
    let (strip_len, strips, bw, bh) = if l.vertical() {
        (cap, 1u32, vw, cap)
    } else {
        let strip_len = w.min(cap).max(len.div_ceil(2));
        let strips = cap.div_ceil(strip_len);
        (strip_len, strips, strip_len, strips * vh)
    };
    (bw <= w && strip_len <= u32::from(u16::MAX) && strips <= u32::from(u8::MAX))
        .then_some((strip_len, strips, bw, bh))
}

impl super::DslApp {
    /// Places the wash patch, every scroll layer's band and every motion layer's pixels in the
    /// store — shelves below the patch: each band a ring of twice its viewport (half a viewport
    /// of margin each way, `MARGIN_MIN` at least), narrower when the store would pass the
    /// surface's rows, then the motion layers at their size. When even the narrowest set does not
    /// fit, the motion layers go first (the app paints them in the base again), then the layer
    /// with the longest viewport goes flat (painted in the base unscrolled, clipped to its
    /// viewport — as past the layer bound) and the rest are placed again. A band or a motion
    /// layer whose place changed holds nothing (it fills, or paints, again). Without a store
    /// (windowd placed none: `NO_STORE`) every layer goes flat. Returns the plan.
    pub(super) fn plan_store(&mut self) -> StorePlan {
        let (w, h) = (self.w, self.h);
        if self.store_refused {
            self.motion_cache.drop_parked();
            for k in 0..super::scroll::MAX_SCROLL_LAYERS {
                self.flatten(k);
            }
            self.place_motions([None; MAX_MOTION_LAYERS]);
            self.tree_dirty = true;
            return StorePlan { rows: h, width: 0 };
        }
        let max_rows = u32::from(lw::MAX_SURFACE_ROWS);
        let top = h + PATCH_ROWS;
        if self.store_alloc.bounds() != (w, top, max_rows) {
            self.store_alloc = StoreAlloc::new(w, top, max_rows);
            // A new allocator holds no place: no parked store is where it was.
            self.motion_cache.drop_parked();
        }
        // A store painted under another theme is not this one's.
        self.motion_cache.drop_parked_off_theme(self.theme_mode);
        let mut motions = self.motion_items();
        let mut layers = self.scroll_layers;
        let mut shapes = [None; super::scroll::MAX_SCROLL_LAYERS];
        loop {
            // A leaving layer's bands keep their places (TASK-0354 P9) while the bound has room.
            let mut ghost_items = [Item { key: 0, w: 0, h: 0 }; MAX_ITEMS];
            let mut ng = 0;
            self.ghost_bands(|key, w, h| {
                if let Some(slot) = ghost_items.get_mut(ng) {
                    *slot = Item { key, w, h };
                    ng += 1;
                }
            });
            // A parked store keeps its place too — the first to give way (`give_way`).
            let mut parked_items = [Item { key: 0, w: 0, h: 0 }; MAX_PARKED];
            let mut np = 0;
            self.motion_cache.parked_items(|key, w, h| {
                if let Some(slot) = parked_items.get_mut(np) {
                    *slot = Item { key, w, h };
                    np += 1;
                }
            });
            let live = layers.iter().flatten().count() + motions.iter().flatten().count();
            if np > 0 && live + ng + np > MAX_ITEMS {
                self.motion_cache.drop_parked();
                continue;
            }
            if live + ng > MAX_ITEMS {
                self.cut_ghosts("bound");
                ng = 0;
                motions = self.motion_items();
            }
            // Every band at the widest margin that fits, then narrower; an item that stays keeps
            // its place (`store_alloc`), so what it drew stays valid.
            let alloc = &mut self.store_alloc;
            let mut outcome = Outcome::Full;
            for pct in [50, 20, 0] {
                let mut items = [Item { key: 0, w: 0, h: 0 }; MAX_ITEMS];
                let mut n = 0;
                let mut shaped = true;
                for (k, l) in layers.iter().enumerate() {
                    shapes[k] = None;
                    let Some(l) = l else { continue };
                    let Some(s) = band_shape(l, pct, w) else {
                        shaped = false;
                        break;
                    };
                    shapes[k] = Some(s);
                    items[n] = Item { key: BAND_KEY | k as u32, w: s.2, h: s.3 };
                    n += 1;
                }
                if !shaped {
                    continue;
                }
                for &(id, (mw, mh)) in motions.iter().flatten() {
                    items[n] = Item { key: LAYER_KEY | u32::from(id), w: mw, h: mh };
                    n += 1;
                }
                items[n..n + ng].copy_from_slice(&ghost_items[..ng]);
                items[n + ng..n + ng + np].copy_from_slice(&parked_items[..np]);
                outcome = alloc.plan(&items[..n + ng + np]);
                if outcome != Outcome::Full {
                    break;
                }
            }
            // A compaction moved every place: a parked store's or a ghost's pixels are no longer
            // where a re-enter or windowd would find them — they go, the rest is planned again.
            // No room: the parked stores give way first, then a ghost — before anything live: the
            // big launcher entering as the small one leaves must be a layer, not the app's paint
            // (board--2026-11-19T09-18-34: four `motion in app (token=4 why=store)` beside a
            // leaving launcher) — then the motion layers (the app paints them in the base again),
            // then the band with the longest viewport.
            let present = (np > 0, self.has_ghosts(), motions.iter().any(Option::is_some));
            match give_way(outcome, present) {
                GiveWay::Placed => break,
                GiveWay::Parked => {
                    self.motion_cache.drop_parked();
                }
                GiveWay::Ghosts(why) => {
                    self.cut_ghosts(why);
                    motions = self.motion_items();
                }
                GiveWay::Motions => motions = [None; MAX_MOTION_LAYERS],
                GiveWay::Band => {
                    let longest = (0..layers.len())
                        .filter(|&k| layers[k].is_some())
                        .max_by_key(|&k| layers[k].map_or(0, |l| l.viewport_len()));
                    match longest {
                        Some(k) => layers[k] = None,
                        None => {
                            let _ = self.store_alloc.plan(&[]);
                            break;
                        }
                    }
                }
            }
        }
        let at = |key: u32| {
            self.store_alloc.place_of(key).map(|(x, y, _, _)| (x as u16, y.min(0xFFFF) as u16))
        };
        let mut bands = [Band::default(); super::scroll::MAX_SCROLL_LAYERS];
        for (k, band) in bands.iter_mut().enumerate() {
            let (Some((strip_len, strips, _, _)), Some((bx, by))) =
                (shapes[k].filter(|_| layers[k].is_some()), at(BAND_KEY | k as u32))
            else {
                continue;
            };
            *band = Band {
                bx,
                by,
                strip_len: strip_len as u16,
                strips: strips as u8,
                valid_start: 0,
                valid_len: 0,
            };
        }
        let mut places: Places = [None; MAX_MOTION_LAYERS];
        for (place, m) in places.iter_mut().zip(motions) {
            *place = m.and_then(|(id, _)| at(LAYER_KEY | u32::from(id)));
        }
        let (rows, used_w) = self.store_alloc.extent();
        let (rows, used_w) = (rows.max(top), used_w.max(PATCH_COLS));
        let mut flattened = 0;
        for (k, kept) in layers.iter().enumerate() {
            if kept.is_none() && self.flatten(k) {
                flattened += 1;
            }
        }
        // Named, never a slow path in disguise: these containers do not scroll.
        if flattened > 0 && crate::proof_line::STORE_FULL.claim() {
            raw_marker(&alloc::format!("apphost: store full (flattened={flattened})"));
        }
        for (slot, band) in self.scroll_layers.iter_mut().zip(bands) {
            if let Some(l) = slot.as_mut() {
                let same = (l.band.bx, l.band.by, l.band.strip_len, l.band.strips)
                    == (band.bx, band.by, band.strip_len, band.strips);
                if !same {
                    l.band = band;
                }
            }
        }
        self.place_motions(places);
        self.patch = (0, h as u16);
        self.tree_dirty = true;
        let n = self.scroll_layers.iter().flatten().count();
        if crate::proof_line::SCROLL_LAYERS.raise(n) {
            raw_marker(&alloc::format!(
                "apphost: scroll layers (n={n} band_rows={})",
                rows - h - PATCH_ROWS
            ));
        }
        StorePlan { rows, width: used_w.min(w) as u16 }
    }

    /// Each motion layer's store item: its id and its size (one of no area takes none).
    fn motion_items(&self) -> [Option<(u16, (u32, u32))>; MAX_MOTION_LAYERS] {
        self.motion_store_items()
    }

    /// Motion slot `k`'s ghost of `identity` and `size` is dropped: its store at `place` parks
    /// under a parked key (`crate::motion_reuse::Cache::park`) — its place kept by the next plan
    /// — when the slot's snapshot describes it; else the place is freed.
    pub(super) fn park_store(&mut self, k: usize, at: (i32, (u32, u32), (u16, u16))) {
        let alloc = &mut self.store_alloc;
        self.motion_cache.park(k, at, PARKED_KEY, |(x, y), key| {
            alloc.rekey_at((u32::from(x), u32::from(y)), key)
        });
    }

    /// A new motion layer `id` of `identity` and `size` in slot `slot` takes a store parked for
    /// them: the item is the layer's (its place kept by the next plan) and the snapshot the
    /// slot's (an item no longer where it was parked leaves the slot unknown).
    pub(super) fn adopt_parked(
        &mut self,
        slot: usize,
        (identity, size): (i32, (u32, u32)),
        id: u16,
    ) {
        let Some(j) = parked_for(self.motion_cache.parked(), identity, size) else { return };
        let Some((x, y)) = self.motion_cache.unpark(j, slot) else { return };
        if !self.store_alloc.rekey_at((u32::from(x), u32::from(y)), LAYER_KEY | u32::from(id)) {
            self.motion_cache.forget(slot);
        }
    }

    /// Layer `k` goes flat: no band — its content paints in the base unscrolled, clipped to its
    /// viewport (as a container past the layer bound). Whether there was a layer.
    pub(super) fn flatten(&mut self, k: usize) -> bool {
        let Some(l) = self.scroll_layers[k].take() else { return false };
        let view = nexus_scene_raster::ScrollView { root: l.root, clip: l.viewport, dx: 0, dy: 0 };
        self.flat_views.push(view);
        true
    }

    /// The surface was made for `plan` (RFC-0114: one slot of its buffer holds the base and the
    /// store) — it fits what was planned: nothing waits for a re-create (every surface was made
    /// twice at start, the second time for nothing).
    pub(super) fn created(&mut self, plan: StorePlan) {
        self.store_rows = plan.rows;
        self.store_w = plan.width;
        self.store_wanted = None;
        self.created_h = self.h;
    }

    /// Whether the store's latest plan waits for a re-create.
    pub(super) fn replan_owed(&self) -> bool {
        self.store_wanted.is_some_and(|p| self.outgrows(p))
    }

    /// [`replan_owed`](Self::replan_owed), the plan taken: the loop asks once.
    pub(super) fn take_replan(&mut self) -> bool {
        self.store_wanted.take().is_some_and(|p| self.outgrows(p))
    }

    /// No memory for the store's buffer (the create answered `QUOTA`): every scroll layer paints
    /// flat until the next create asks again. Returns the flat plan to create with.
    pub(super) fn refuse_store(&mut self) -> StorePlan {
        self.store_refused = true;
        let plan = self.plan_store();
        if crate::proof_line::STORE_REFUSED.claim() {
            raw_marker("apphost: store refused (no memory for the bands: scroll layers flat)");
        }
        plan
    }

    /// The store a re-create asks for: one refused before is asked for again — its layers back
    /// from flat.
    /// At the height the surface was made a re-create never asks for less than it holds (a
    /// store that shrank and grew again was made twice). A surface whose page base is clear —
    /// the desktop, where every entrance is a motion layer — asks for the most rows at its width
    /// once, so opening a layer never makes it again (TASK-0354 P8: a re-create cost 0.3-0.45 s
    /// on the board); a `QUOTA` answer to that asks for the plan alone (`reserve_refused`).
    pub(super) fn plan_for_create(&mut self) -> StorePlan {
        if core::mem::take(&mut self.store_refused) {
            let _ = self.sync_layers();
        }
        let plan = self.plan_store();
        let held = if self.h == self.created_h { (self.store_rows, self.store_w) } else { (0, 0) };
        let plan = StorePlan { rows: plan.rows.max(held.0), width: plan.width.max(held.1) };
        if self.page_base_alpha() != 0 || self.reserve_refused || self.store_refused {
            return plan;
        }
        let full = u16::try_from(self.w).unwrap_or(u16::MAX);
        StorePlan { rows: plan.rows.max(u32::from(lw::MAX_SURFACE_ROWS)), width: full }
    }

    /// The create refused the reserved store for want of memory: the plan alone, once.
    pub(super) fn unreserve(&mut self, asked: StorePlan) -> Option<StorePlan> {
        if self.reserve_refused || self.page_base_alpha() != 0 {
            return None;
        }
        self.reserve_refused = true;
        let plan = self.plan_for_create();
        (plan != asked).then_some(plan)
    }

    /// The surface was made anew at its size. Its marker, and what the re-create costs (TASK-0354
    /// P8): whether only the store grew, the rows that came over from the old buffer, and what
    /// the next present paints of the base (`whole`: every row again; else the pending
    /// rectangles — a motion layer's rest among them).
    /// `why`: what the old surface lacked — `rows`/`width` (the store outgrew it), `resize` (a
    /// new size), `lost` (the compositor went); `held`: what it had; `us`: the create's time.
    pub(super) fn say_resized(&self, store_only: bool, held: StorePlan, us: u64) {
        let why = match (store_only, held.rows < self.store_rows) {
            (false, _) => "resize",
            (true, true) => "rows",
            (true, false) => "width",
        };
        raw_marker("apphost: resized");
        raw_marker(&alloc::format!(
            "apphost: surface re-made (store_only={} why={why} plan={}x{} held={}x{} us={us} \
             carried={} base_paints={})",
            u8::from(store_only),
            self.store_rows,
            self.store_w,
            held.rows,
            held.width,
            self.carried.unwrap_or(0),
            self.pending
        ));
    }

    /// Whether `plan` needs more than the surface was created with (a re-create).
    pub(super) fn outgrows(&self, plan: StorePlan) -> bool {
        plan.rows > self.store_rows || plan.width > self.store_w
    }
}
