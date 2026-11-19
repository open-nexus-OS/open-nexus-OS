// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a motion layer LEAVING (RFC-0109 §motion layers, ADR-0097 amendment "Exits",
//! TASK-0354 P9). When a layer's node leaves the scene, the layer stays as a GHOST for its exit:
//! it holds no node, keeps its id (the layer's life), its place in the store, its backdrop and the
//! last declarations of the regions and scroll layers it claimed, and is declared with
//! `MOTION_EXIT` — windowd plays the token back toward where it came from. The app drops it on its
//! own one-shot timer at the exit's end plus a margin: windowd never tells a client (ADR-0075).
//! Nothing writes a ghost's store pixels; a whole redraw or a surface re-create cuts the ghosts
//! first. The same node coming back within the exit revives the same id.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the store's stable places (`crate::store_alloc`), the wire (`nexus-display-proto`
//!   `a_leaving_motion_layer_round_trips`); the lanes' `apphost: motion exit (` and windowd's
//!   `windowd: motion layer (… exit=1`

use super::*;
use nexus_display_proto::client_surface as wire;
use nexus_display_proto::surface_layers as lw;

/// The regions a ghost keeps declaring for its exit (past it, the extra ones rest at once).
const LEAVE_REGIONS: usize = 4;
/// The scroll layers a ghost keeps declaring (their bands keep their store places).
const LEAVE_SCROLLS: usize = 2;
/// A ghost band's key in the store's places: its layer's slot and its index.
const GHOST_BAND_KEY: u32 = 1 << 18;
/// A motion is over this long after its end (shared with the enter's "at rest" declaration).
pub(super) const OVER_AFTER_NS: u64 = 250_000_000;

/// What a leaving layer keeps for its exit.
#[derive(Clone, Copy)]
pub(super) struct Leaving {
    /// When the app drops it (monotonic ns): the exit's end (its scrim's, if longer) plus
    /// `OVER_AFTER_NS`.
    until_ns: u64,
    regions: [wire::LayerDesc; LEAVE_REGIONS],
    nr: usize,
    /// Its scroll layers' last declarations and offsets.
    scrolls: [(lw::ScrollLayerDesc, i32); LEAVE_SCROLLS],
    ns: usize,
}

/// The claims a structure gives each slot's ghost: region bits and scroll bits.
pub(in crate::probe) type GhostClaims = [(u16, u8); MAX_MOTION_LAYERS];

impl crate::probe::DslApp {
    /// Motion layer `k`'s node left the scene: it leaves as a ghost when it was drawn and moves
    /// (a reduced-motion theme, a layer never shown, a ghost already: gone at once). Whether it
    /// stays for an exit.
    pub(super) fn leave(&mut self, k: usize) -> bool {
        let Some(mut m) = self.motion_layers[k] else { return false };
        if m.duration_ms == 0 || m.store.is_none() || !m.painted || m.leaving.is_some() {
            return false;
        }
        let sent = self.sent_tree;
        let Some(d) = sent.motions().iter().find(|d| d.id == m.id).copied() else { return false };
        let mut l = Leaving {
            until_ns: 0,
            regions: [wire::LayerDesc::default(); LEAVE_REGIONS],
            nr: 0,
            scrolls: [(lw::ScrollLayerDesc::default(), 0); LEAVE_SCROLLS],
            ns: 0,
        };
        let mut rested = 0;
        for (j, r) in sent.regions().iter().enumerate().filter(|(j, _)| d.claims_region(*j)) {
            let _ = j;
            match l.regions.get_mut(l.nr) {
                Some(slot) => {
                    *slot = *r;
                    l.nr += 1;
                }
                None => rested += 1,
            }
        }
        for (j, s) in sent.scrolls().iter().enumerate().filter(|(j, _)| d.claims_scroll(*j)) {
            let _ = j;
            let offset = self
                .scroll_layers
                .iter()
                .flatten()
                .find(|x| (x.band.bx, x.band.by) == (s.bx, s.by))
                .map_or(0, |x| x.offset);
            let key = GHOST_BAND_KEY | (k as u32) << 2 | l.ns as u32;
            let at = (u32::from(s.bx), u32::from(s.by));
            if l.ns < LEAVE_SCROLLS && self.store_alloc.rekey_at(at, key) {
                l.scrolls[l.ns] = (*s, offset);
                l.ns += 1;
            } else {
                rested += 1;
            }
        }
        let scrim = if m.backdrop.is_some() { self.scrim_ms() } else { 0 };
        let now = nsec_now();
        l.until_ns = crate::motion_exit::ghost_until(now, m.duration_ms, scrim, OVER_AFTER_NS);
        (m.root, m.end, m.declared_ns, m.leaving) = (0, 0, now, Some(l));
        self.motion_layers[k] = Some(m);
        self.tree_dirty = true;
        let (w, h) = m.size();
        raw_marker(&alloc::format!(
            "apphost: motion exit (id={} token={} w={w} h={h} ms={} kept={}/{} rested={rested})",
            m.id,
            m.token,
            m.duration_ms,
            l.nr,
            l.ns
        ));
        true
    }

    /// The leaving layer whose node comes back (`identity`): its slot — the same id turns where
    /// it is (windowd keeps the position and velocity).
    fn revive(&mut self, identity: i32, (first, end): (usize, usize)) -> Option<usize> {
        let mut layers = [None; MAX_MOTION_LAYERS];
        for (l, m) in layers.iter_mut().zip(self.motion_layers.iter()) {
            *l = m.map(|m| (m.identity, m.leaving.is_some()));
        }
        let k = crate::motion_exit::revives(&layers, identity)?;
        let m = self.motion_layers[k].as_mut()?;
        (m.root, m.end, m.leaving, m.painted) = (first, end, None, false);
        Some(k)
    }

    /// A `.transition` entering on `node_id` whose layer is still leaving (`identity`): the same
    /// layer turns back — its id kept, so windowd keeps the position and velocity — its pixels
    /// paint once more at its place, a new size is placed again. `None`: no ghost of it.
    pub(super) fn revive_as(
        &mut self,
        (node_id, identity): (usize, i32),
        token: animation::MotionToken,
        duration_ms: u16,
    ) -> Option<Result<(), &'static str>> {
        let span = crate::motion_math::subtree_span(self.view.scene(), node_id)?;
        let k = self.revive(identity, span)?;
        let shape = self.layer_shape(span, k);
        if let (Some(m), Some((rect, backdrop))) = (self.motion_layers[k].as_mut(), shape) {
            let size = |r: crate::motion_math::Span| (r.2 - r.0, r.3 - r.1);
            let grew = size(rect) != size(m.rect);
            (m.rect, m.backdrop, m.token, m.duration_ms) =
                (rect, backdrop, token.id(), duration_ms);
            m.declared_ns = nsec_now();
            if grew {
                let plan = self.plan_store();
                self.store_wanted = Some(plan);
            }
        }
        self.tree_dirty = true;
        Some(self.motion_layers[k].and_then(|m| m.store).map(|_| ()).ok_or("store"))
    }

    /// The earliest drop of a ghost (monotonic ns), if one waits.
    pub(in crate::probe) fn ghost_deadline_ns(&self) -> Option<u64> {
        self.motion_layers.iter().flatten().filter_map(|m| m.leaving.map(|l| l.until_ns)).min()
    }

    /// Drops every ghost whose exit is over at `now`. Whether one went.
    pub(in crate::probe) fn drop_due_ghosts(&mut self, now: u64) -> bool {
        let mut any = false;
        for slot in self.motion_layers.iter_mut() {
            if slot.is_some_and(|m| {
                m.leaving.is_some_and(|l| crate::motion_exit::ghost_due(l.until_ns, now))
            }) {
                *slot = None;
                any = true;
            }
        }
        if any {
            self.tree_dirty = true;
            let plan = self.plan_store();
            self.store_wanted = Some(plan);
        }
        any
    }

    /// Every ghost goes at once (`why`: a whole redraw, a surface re-create, a full store) —
    /// nothing may paint over a ghost's pixels while windowd still draws them.
    pub(in crate::probe) fn cut_ghosts(&mut self, why: &str) {
        let mut n = 0;
        for slot in self.motion_layers.iter_mut() {
            if slot.is_some_and(|m| m.leaving.is_some()) {
                *slot = None;
                n += 1;
            }
        }
        if n > 0 {
            self.tree_dirty = true;
            raw_marker(&alloc::format!("apphost: exit cut (why={why} n={n})"));
        }
    }

    /// Whether a ghost waits for its exit.
    pub(in crate::probe) fn has_ghosts(&self) -> bool {
        self.motion_layers.iter().flatten().any(|m| m.leaving.is_some())
    }

    /// The ghost bands the store keeps (`key`, width, height).
    pub(in crate::probe) fn ghost_bands(&self, mut each: impl FnMut(u32, u32, u32)) {
        for (k, m) in self.motion_layers.iter().enumerate() {
            let Some(l) = m.and_then(|m| m.leaving) else { continue };
            for (j, (s, _)) in l.scrolls[..l.ns].iter().enumerate() {
                let (_, _, w, h) = s.band_rect();
                each(GHOST_BAND_KEY | (k as u32) << 2 | j as u32, w, h);
            }
        }
    }

    /// The structure's regions: the page's, then the ghosts' kept ones while the bound has room.
    /// `nodes[i]` is region `i`'s node (`usize::MAX`: a ghost's). Returns the count and each
    /// slot's ghost region bits.
    pub(in crate::probe) fn tree_regions(
        &self,
        out: &mut [wire::LayerDesc; wire::MAX_SURFACE_LAYERS],
        nodes: &mut [usize; wire::MAX_SURFACE_LAYERS],
    ) -> (usize, GhostClaims) {
        let mut n = self.regions(out, nodes);
        let mut claims: GhostClaims = [(0, 0); MAX_MOTION_LAYERS];
        for (k, m) in self.motion_layers.iter().enumerate() {
            let Some(l) = m.and_then(|m| m.leaving) else { continue };
            for r in &l.regions[..l.nr] {
                if n >= wire::MAX_SURFACE_LAYERS || n >= 16 {
                    break;
                }
                (out[n], nodes[n]) = (*r, usize::MAX);
                claims[k].0 |= 1 << n;
                n += 1;
            }
        }
        (n, claims)
    }

    /// The ghosts' kept scroll layers appended to the structure's from `from` (and their offsets
    /// to `offsets`), while the bound has room. Returns the count and adds each slot's bits.
    pub(in crate::probe) fn ghost_scrolls(
        &self,
        from: usize,
        out: &mut [lw::ScrollLayerDesc; lw::MAX_SCROLL_LAYERS],
        offsets: &mut [i32; lw::MAX_SCROLL_LAYERS],
        claims: &mut GhostClaims,
    ) -> usize {
        let mut n = from;
        for (k, m) in self.motion_layers.iter().enumerate() {
            let Some(l) = m.and_then(|m| m.leaving) else { continue };
            for &(s, offset) in &l.scrolls[..l.ns] {
                if n >= lw::MAX_SCROLL_LAYERS {
                    break;
                }
                (out[n], offsets[n]) = (s, offset);
                claims[k].1 |= 1 << n;
                n += 1;
            }
        }
        n
    }
}
