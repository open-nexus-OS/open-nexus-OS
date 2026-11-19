// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the motion layers on the wire (RFC-0109 §motion layers, TASK-0354 P2/P9): their paint
//! order (the app's pre-order — a modal after the panel it may cover), each placed layer's
//! descriptor with what it claims, its motion (at rest once over; a ghost never at rest, declared
//! with `MOTION_EXIT`), whether it is in front and its backdrop. Split out of `motion_layers.rs`
//! (module-size ratchet).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the wire's bounds in `nexus-display-proto::surface_layers`; the lanes'
//!   `windowd: motion layer (`

use super::leave::{GhostClaims, OVER_AFTER_NS};
use super::*;
use nexus_display_proto::surface_layers as lw;

impl crate::probe::DslApp {
    /// The placed motion layers' slots in the app's paint order (pre-order: a modal after the
    /// panel it may cover) — the order on the wire, the order windowd draws them in.
    fn wire_order(&self) -> ([usize; MAX_MOTION_LAYERS], usize) {
        let mut out = [0; MAX_MOTION_LAYERS];
        let mut n = 0;
        for (k, m) in self.motion_layers.iter().enumerate() {
            if m.is_some_and(|m| m.store.is_some()) {
                out[n] = k;
                n += 1;
            }
        }
        let roots = |k: usize| self.motion_layers[k].map_or(usize::MAX, |m| m.root);
        out[..n].sort_unstable_by_key(|&k| roots(k));
        (out, n)
    }

    /// The structure's motion layers into `out`, in paint order: each placed one, what it claims
    /// — `regions`: the node of each region the structure declares; `scrolls`: each scroll layer's
    /// root in the structure's order — its motion (at rest once it is over), whether it is in
    /// front, its backdrop. Returns the count.
    pub(in crate::probe) fn motion_descs(
        &self,
        (regions, scrolls): (&[usize], &[usize]),
        ghosts: &GhostClaims,
        out: &mut [lw::MotionLayerDesc; MAX_MOTION_LAYERS],
    ) -> usize {
        let now = nsec_now();
        let (order, n) = self.wire_order();
        let scrim_ms = self.scrim_ms();
        for (slot, &k) in out.iter_mut().zip(&order[..n]) {
            let Some(m) = self.motion_layers[k] else { continue };
            let Some((sx, sy)) = m.store else { continue };
            let mask = |ids: &[usize]| {
                ids.iter()
                    .enumerate()
                    .filter(|(_, &id)| m.holds(id))
                    .fold(0u32, |a, (k, _)| a | 1 << k)
            };
            // A ghost is never declared at rest: windowd would draw it there (its exit's end is
            // its drop, `probe/motion_leave.rs`).
            let leaving = m.leaving.is_some();
            let over = !leaving
                && now > m.declared_ns + u64::from(m.duration_ms) * 1_000_000 + OVER_AFTER_NS;
            let (claimed_regions, claimed_scrolls) = if leaving {
                (u32::from(ghosts[k].0), u32::from(ghosts[k].1))
            } else {
                (mask(regions), mask(scrolls))
            };
            let (w, h) = m.size();
            let backdrop = m.backdrop.zip(backdrop_patch(k));
            *slot = lw::MotionLayerDesc {
                x: m.rect.0 as u16,
                y: m.rect.1 as u16,
                w: w as u16,
                h: h as u16,
                sx,
                sy,
                id: m.id,
                token: m.token,
                duration_ms: if over { 0 } else { m.duration_ms },
                from_dx: m.from.0,
                from_dy: m.from.1,
                // A layer leaves fading out whatever it entered with: one that only slid in would
                // stand opaque at its exit's end and vanish there (a pop).
                from_opacity: if leaving { 0 } else { m.from.2 },
                from_scale: if m.from.3 == 10_000 { 0 } else { m.from.3 },
                regions: claimed_regions as u16,
                scrolls: claimed_scrolls as u8,
                front: m.front,
                exit: leaving,
                backdrop: backdrop.map_or(0, |(_, p)| p as u8 + 1),
                backdrop_opacity: backdrop.map_or(0, |(c, _)| c.a),
                // A scrim arrives in the theme's quick step (the sheet springs in its own).
                backdrop_ms: if backdrop.is_some() && !over { scrim_ms } else { 0 },
            };
        }
        n
    }

    /// The theme's quick step (ms): a scrim arrives and leaves in it.
    pub(super) fn scrim_ms(&self) -> u16 {
        tokens_for(self.theme_mode)
            .motion_ms(nexus_dsl_runtime::theme_tokens::MotionDurationToken::Quick)
            .min(u32::from(lw::MAX_MOTION_MS)) as u16
    }

    /// The wire place of motion layer `k` among the placed ones (a wash rides it by that number).
    pub(in crate::probe) fn motion_wire_index(&self, k: usize) -> Option<u8> {
        let (order, n) = self.wire_order();
        order[..n].iter().position(|&j| j == k).map(|i| i as u8)
    }
}
