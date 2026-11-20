// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a surface's regions keep their layer key while they stay what they are (TASK-0351
//! P6, RFC-0101 §8): a region's key was its index in the declared list, so a region that came or
//! went before another shifted every later key — the planner saw those regions gone and new and
//! wrote their glass. Each structure now carries the ids: an equal region keeps its id, a region
//! changed in place keeps its slot's, a new one takes the lowest free id; two regions never share
//! one. With nothing changed the ids are the indices (the keys of before).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below

use nexus_display_proto::client_surface::{LayerDesc, MAX_SURFACE_LAYERS};

/// A surface's region ids, one per declared region (`ids[i]` names region `i`).
pub(crate) type RegionIds = [u8; MAX_SURFACE_LAYERS];

/// The ids of a surface that declared nothing yet: the indices.
pub(crate) const IDENTITY: RegionIds = {
    let mut ids = [0u8; MAX_SURFACE_LAYERS];
    let mut i = 0;
    while i < MAX_SURFACE_LAYERS {
        ids[i] = i as u8;
        i += 1;
    }
    ids
};

/// The ids of `next`'s regions, carried from `now`'s (`now_ids`).
pub(crate) fn carry(now: &[LayerDesc], now_ids: &RegionIds, next: &[LayerDesc]) -> RegionIds {
    const NONE: u8 = u8::MAX;
    let mut ids = [NONE; MAX_SURFACE_LAYERS];
    let mut used = [false; MAX_SURFACE_LAYERS];
    let mut held = [false; MAX_SURFACE_LAYERS];
    let now = &now[..now.len().min(MAX_SURFACE_LAYERS)];
    let next = &next[..next.len().min(MAX_SURFACE_LAYERS)];
    let take = |j: usize, i: usize, ids: &mut RegionIds, used: &mut [bool], held: &mut [bool]| {
        let id = now_ids[i];
        if usize::from(id) < MAX_SURFACE_LAYERS && !used[usize::from(id)] {
            ids[j] = id;
            used[usize::from(id)] = true;
            held[i] = true;
        }
    };
    // An equal region keeps its id, wherever it moved in the list.
    for (j, b) in next.iter().enumerate() {
        if let Some(i) = (0..now.len()).find(|&i| !held[i] && now[i] == *b) {
            take(j, i, &mut ids, &mut used, &mut held);
        }
    }
    // A region changed in place (its slot unmatched on both sides) keeps that slot's id.
    for j in 0..next.len() {
        if ids[j] == NONE && j < now.len() && !held[j] {
            take(j, j, &mut ids, &mut used, &mut held);
        }
    }
    // The rest take the lowest free id.
    for id in ids.iter_mut().take(next.len()) {
        if *id == NONE {
            if let Some(free) = (0..MAX_SURFACE_LAYERS).find(|&k| !used[k]) {
                *id = free as u8;
                used[free] = true;
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(x: u16) -> LayerDesc {
        LayerDesc { x, y: 10, w: 100, h: 40, ..LayerDesc::default() }
    }

    /// The architect's fact (build/agents/arch-transition-every-frame-2026-11-19.md, Decision 3):
    /// regions [A, B] → [B] — B keeps its key.
    #[test]
    fn a_region_keeps_its_key_when_one_before_it_goes() {
        let (a, b) = (region(0), region(200));
        let ids = carry(&[a, b], &IDENTITY, &[b]);
        assert_eq!(ids[0], 1, "B keeps id 1 although it is now first");
    }

    #[test]
    fn nothing_changed_keeps_the_indices() {
        let r = [region(0), region(200), region(400)];
        assert_eq!(&carry(&r, &IDENTITY, &r)[..3], &[0, 1, 2]);
    }

    #[test]
    fn a_region_changed_in_place_keeps_its_slots_id_and_a_new_one_takes_a_free_id() {
        let (a, b, c) = (region(0), region(200), region(400));
        let moved = LayerDesc { w: 120, ..b };
        let ids = carry(&[a, b], &IDENTITY, &[a, moved, c]);
        assert_eq!(&ids[..3], &[0, 1, 2]);
        // A comes back after it went: it takes a free id, B keeps its own.
        let gone = carry(&[a, b], &IDENTITY, &[b]);
        let back = carry(&[b], &gone, &[a, b]);
        assert_eq!((back[0], back[1]), (0, 1));
    }

    #[test]
    fn test_reject_two_regions_sharing_an_id() {
        let r: [LayerDesc; 6] = core::array::from_fn(|k| region(k as u16 * 50));
        let mut ids = IDENTITY;
        let mut now = r.to_vec();
        for step in 0..12usize {
            let mut next = now.clone();
            if step % 3 == 0 && !next.is_empty() {
                next.remove(step % next.len());
            } else {
                next.push(region(1000 + step as u16));
            }
            ids = carry(&now, &ids, &next);
            let mut seen = [false; MAX_SURFACE_LAYERS];
            for &id in &ids[..next.len()] {
                assert!(usize::from(id) < MAX_SURFACE_LAYERS && !seen[usize::from(id)], "{ids:?}");
                seen[usize::from(id)] = true;
            }
            now = next;
        }
    }
}
