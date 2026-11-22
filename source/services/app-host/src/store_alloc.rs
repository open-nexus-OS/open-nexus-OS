// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: where the store's items live (RFC-0109, TASK-0354 P9) — the rectangles below a
//! surface's base that hold the scroll layers' bands and the motion layers' pixels. An item keeps
//! its place while others come and go (a leaving motion layer's pixels must not move under its
//! exit, and a band that stays keeps what it drew); a new or resized item takes the lowest, then
//! leftmost, free place (bottom-left first fit over the placed rectangles' edges); only when
//! nothing fits are all items placed again from scratch (a compaction — the caller first drops
//! what it can lose, the ghosts). Arithmetic only; the store's planner (`probe/store.rs`) asks it.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below

/// The most items a store holds: every scroll layer's band and every motion layer's pixels, with
/// room for the ghosts of layers leaving beside their successors and for the stores parked by
/// dropped ghosts (`crate::motion_reuse::MAX_PARKED`; `probe/store.rs` asserts the sum).
pub(crate) const MAX_ITEMS: usize = 16;

/// One item asked for: its identity (stable across plans) and its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub(crate) key: u32,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

/// One placed item: its identity and its rectangle `(x, y, w, h)` in surface rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Placed {
    pub(crate) key: u32,
    pub(crate) rect: (u32, u32, u32, u32),
}

/// What a plan did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Every item fits; items that stayed kept their places.
    Kept,
    /// Nothing fit around the items that stayed: every item was placed again (places moved).
    Compacted,
    /// The items do not fit at all, even from scratch.
    Full,
}

/// The store's places: `width` columns from row `top` to row `limit` (exclusive).
#[derive(Clone, Copy, Debug)]
pub(crate) struct StoreAlloc {
    placed: [Option<Placed>; MAX_ITEMS],
    width: u32,
    top: u32,
    limit: u32,
}

const fn overlaps(a: (u32, u32, u32, u32), b: (u32, u32, u32, u32)) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

impl StoreAlloc {
    pub(crate) const fn new(width: u32, top: u32, limit: u32) -> Self {
        Self { placed: [None; MAX_ITEMS], width, top, limit }
    }

    /// The store's bounds `(width, top, limit)`: a planner whose bounds changed (a resize, a new
    /// base height) starts a new allocator — nothing keeps its place.
    pub(crate) const fn bounds(&self) -> (u32, u32, u32) {
        (self.width, self.top, self.limit)
    }

    /// Where item `key` is placed.
    pub(crate) fn place_of(&self, key: u32) -> Option<(u32, u32, u32, u32)> {
        self.placed.iter().flatten().find(|p| p.key == key).map(|p| p.rect)
    }

    /// The item placed at `(x, y)` takes the key `key` and keeps its place (a band whose scroll
    /// layer left goes on as a leaving layer's ghost band, TASK-0354 P9). Whether one was there.
    pub(crate) fn rekey_at(&mut self, (x, y): (u32, u32), key: u32) -> bool {
        match self.placed.iter_mut().flatten().find(|p| (p.rect.0, p.rect.1) == (x, y)) {
            Some(p) => {
                p.key = key;
                true
            }
            None => false,
        }
    }

    /// The rows the placed items reach (`top` when none) and the columns they reach.
    pub(crate) fn extent(&self) -> (u32, u32) {
        self.placed.iter().flatten().fold((self.top, 0), |(rows, cols), p| {
            (rows.max(p.rect.1 + p.rect.3), cols.max(p.rect.0 + p.rect.2))
        })
    }

    /// Places `want` (at most `MAX_ITEMS`; keys distinct): an item placed before at the same size
    /// keeps its place, an item no longer asked for frees its place, the rest take the lowest,
    /// then leftmost, free place. When that fails, every item is placed again from scratch
    /// (tallest first); when that fails too, nothing changes and the answer is `Full`.
    pub(crate) fn plan(&mut self, want: &[Item]) -> Outcome {
        if want.len() > MAX_ITEMS || want.iter().any(|i| i.w == 0 || i.h == 0 || i.w > self.width) {
            return Outcome::Full;
        }
        let mut next = [None; MAX_ITEMS];
        let mut n = 0;
        for item in want {
            if let Some(p) = self.placed.iter().flatten().find(|p| p.key == item.key) {
                if (p.rect.2, p.rect.3) == (item.w, item.h) {
                    next[n] = Some(*p);
                    n += 1;
                }
            }
        }
        let mut fresh = true;
        for item in want {
            if next[..n].iter().flatten().any(|p| p.key == item.key) {
                continue;
            }
            match self.free_place(&next[..n], item.w, item.h) {
                Some((x, y)) => {
                    next[n] = Some(Placed { key: item.key, rect: (x, y, item.w, item.h) });
                    n += 1;
                }
                None => {
                    fresh = false;
                    break;
                }
            }
        }
        if fresh {
            self.placed = next;
            return Outcome::Kept;
        }
        // Compaction: every item from scratch, the tallest first (a shelf of tall bands, the
        // small layers in the gaps beside them).
        let mut order = [0usize; MAX_ITEMS];
        for (k, slot) in order.iter_mut().enumerate().take(want.len()) {
            *slot = k;
        }
        let order = &mut order[..want.len()];
        order.sort_by_key(|&k| (core::cmp::Reverse(want[k].h), core::cmp::Reverse(want[k].w)));
        let mut scratch = [None; MAX_ITEMS];
        for (m, &k) in order.iter().enumerate() {
            let item = want[k];
            let Some((x, y)) = self.free_place(&scratch[..m], item.w, item.h) else {
                return Outcome::Full;
            };
            scratch[m] = Some(Placed { key: item.key, rect: (x, y, item.w, item.h) });
        }
        self.placed = scratch;
        Outcome::Compacted
    }

    /// The lowest, then leftmost, place for a `w × h` item among `placed` inside the store: the
    /// candidates are the store's corner and the right and bottom edges of what is placed.
    fn free_place(&self, placed: &[Option<Placed>], w: u32, h: u32) -> Option<(u32, u32)> {
        let mut best: Option<(u32, u32)> = None;
        let xs = core::iter::once(0).chain(placed.iter().flatten().map(|p| p.rect.0 + p.rect.2));
        for x in xs {
            if x + w > self.width {
                continue;
            }
            let ys = core::iter::once(self.top)
                .chain(placed.iter().flatten().map(|p| p.rect.1 + p.rect.3));
            for y in ys {
                if y < self.top || y + h > self.limit {
                    continue;
                }
                if best.is_some_and(|(bx, by)| (by, bx) <= (y, x)) {
                    continue;
                }
                let rect = (x, y, w, h);
                if placed.iter().flatten().all(|p| !overlaps(p.rect, rect)) {
                    best = Some((x, y));
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: u32, w: u32, h: u32) -> Item {
        Item { key, w, h }
    }

    fn no_overlap_inside(a: &StoreAlloc) {
        let placed: std::vec::Vec<_> = a.placed.iter().flatten().collect();
        for (i, p) in placed.iter().enumerate() {
            let (x, y, w, h) = p.rect;
            assert!(x + w <= a.width && y >= a.top && y + h <= a.limit, "inside: {p:?}");
            for q in &placed[i + 1..] {
                assert!(!overlaps(p.rect, q.rect), "{p:?} overlaps {q:?}");
            }
        }
    }

    /// TASK-0354 P9: a layer leaving keeps its pixels where they are while a band goes and
    /// another layer comes — its exit must not watch its store move.
    #[test]
    fn an_item_keeps_its_place_while_others_come_and_go() {
        let mut a = StoreAlloc::new(1920, 1100, 3072);
        assert_eq!(
            a.plan(&[item(1, 1600, 900), item(2, 330, 600), item(3, 400, 120)]),
            Outcome::Kept
        );
        let (band, panel) = (a.place_of(1), a.place_of(2));
        assert_eq!(a.bounds(), (1920, 1100, 3072));
        // The panel does not fit beside the band (1600 + 330 > 1920): it lies below it.
        assert_eq!(a.extent(), (1100 + 900 + 600, 1600), "the rows and columns reached");
        // The band goes, a toast comes: the panel stays.
        assert_eq!(a.plan(&[item(2, 330, 600), item(4, 360, 80)]), Outcome::Kept);
        assert_eq!(a.place_of(2), panel, "the panel did not move");
        assert_eq!(a.place_of(1), None);
        // The toast takes the lowest free place — where the band was, not below everything.
        assert_eq!(a.place_of(4).map(|r| (r.0, r.1)), band.map(|r| (r.0, r.1)));
        no_overlap_inside(&a);
        // A resized item is placed anew; the others stay.
        assert_eq!(a.plan(&[item(2, 330, 600), item(4, 360, 200)]), Outcome::Kept);
        assert_eq!(a.place_of(2), panel);
        no_overlap_inside(&a);
        // A band handed on under another key keeps its place.
        let toast = a.place_of(4).expect("placed");
        assert!(a.rekey_at((toast.0, toast.1), 9));
        assert_eq!(a.plan(&[item(2, 330, 600), item(9, 360, 200)]), Outcome::Kept);
        assert_eq!(a.place_of(9), Some(toast), "the same place under its new key");
        assert!(!a.rekey_at((1, 1), 10), "nothing placed there");
    }

    /// Whatever comes and goes, no two places share a pixel and every place lies in the store.
    #[test]
    fn test_reject_overlapping_places() {
        let mut seed = 0x2545_f491u32;
        let mut next = |below: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % below
        };
        let mut a = StoreAlloc::new(1920, 1100, 3072);
        for _ in 0..2_000 {
            let count = next(MAX_ITEMS as u32 + 1) as usize;
            let mut want = std::vec::Vec::new();
            for k in 0..count {
                let key = next(20);
                if want.iter().any(|i: &Item| i.key == key) {
                    continue;
                }
                let _ = k;
                want.push(item(key, 1 + next(1000), 1 + next(500)));
            }
            let before = a.placed;
            match a.plan(&want) {
                Outcome::Full => assert_eq!(a.placed, before, "a refused plan changes nothing"),
                _ => {
                    for i in &want {
                        assert_eq!(a.place_of(i.key).map(|r| (r.2, r.3)), Some((i.w, i.h)));
                    }
                }
            }
            no_overlap_inside(&a);
        }
    }

    /// When nothing fits around what stays, everything is placed again from scratch; when that
    /// does not fit either, the plan is refused and the places are untouched.
    #[test]
    fn no_room_compacts_from_scratch() {
        let mut a = StoreAlloc::new(100, 0, 100);
        let quarters = [item(1, 50, 50), item(2, 50, 50), item(3, 50, 50), item(4, 50, 50)];
        assert_eq!(a.plan(&quarters), Outcome::Kept);
        // Two diagonal quarters stay: a full-width item fits nowhere around them, but from
        // scratch the two share a row and it takes the other.
        let want = [item(1, 50, 50), item(4, 50, 50), item(5, 100, 50)];
        assert_eq!(a.plan(&want), Outcome::Compacted);
        no_overlap_inside(&a);
        assert!(a.place_of(5).is_some());
        let before = a.placed;
        assert_eq!(a.plan(&[item(1, 100, 100), item(2, 10, 10)]), Outcome::Full);
        assert_eq!(a.placed, before, "a refused plan changes nothing");
    }
}
