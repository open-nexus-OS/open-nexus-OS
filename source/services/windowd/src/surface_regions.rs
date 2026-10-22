// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: where a client surface's pixels live in the atlas (RFC-0109). A surface is
//! `width × rows`: rows `[0, height)` its BASE, copied into the base's region; rows
//! `[height, rows)` its STORE, of which columns `[0, store_w)` are used, copied into a region of
//! their own — placed wherever the atlas has room, so a store that grows never needs the base's
//! neighbourhood, and the base keeps its place. The one mapping the row copy and the layer encoder
//! share: a surface row or a store pixel to its atlas place, nothing past what is placed.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below

use crate::atlas::AtlasSurface;
use crate::client_surface::ClientSurface;

/// One placed region: its first absolute row, its column and the columns used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Region {
    row: u32,
    col: u32,
    width: u32,
}

/// A surface's atlas regions, as its copy and its layers address them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SurfaceRegions {
    /// The rows a copy may name: the base's, and the store's when placed.
    pub(crate) rows: u32,
    /// The surface's base height: rows below it are the store's.
    height: u32,
    base: Region,
    store: Option<Region>,
}

impl SurfaceRegions {
    /// The regions of `client` placed at `base` (and `store`): the base's rows its height, the
    /// store's the rows below it — each cut to what its region holds.
    pub(crate) fn of(
        client: &ClientSurface,
        base: AtlasSurface,
        store: Option<AtlasSurface>,
    ) -> Self {
        let height = u32::from(client.height).min(base.height);
        let width = u32::from(client.width).min(base.width);
        let store_rows = u32::from(client.rows.saturating_sub(client.height));
        let store = store.filter(|_| store_rows > 0 && client.store_w > 0).map(|s| {
            let rows = store_rows.min(s.height);
            (
                Region { row: s.abs_row, col: s.x, width: u32::from(client.store_w).min(s.width) },
                rows,
            )
        });
        Self {
            rows: height + store.map_or(0, |(_, rows)| rows),
            height,
            base: Region { row: base.abs_row, col: base.x, width },
            store: store.map(|(r, _)| r),
        }
    }

    /// Surface row `y` in the atlas: its absolute row, its column and the columns to copy;
    /// `None` past what is placed.
    pub(crate) fn place(&self, y: u32) -> Option<(u32, u32, u32)> {
        if y < self.height {
            return Some((self.base.row + y, self.base.col, self.base.width));
        }
        let s = self.store.filter(|_| y < self.rows)?;
        Some((s.row + (y - self.height), s.col, s.width))
    }

    /// The atlas place of store pixel `(x, y)` (surface coordinates, `y` at or below the base's
    /// height): its absolute row and column — what a slice or a wash samples. `None` outside the
    /// placed store.
    pub(crate) fn store_place(&self, x: u32, y: u32) -> Option<(u32, u32)> {
        let s = self.store.filter(|s| y >= self.height && y < self.rows && x < s.width)?;
        Some((s.row + (y - self.height), s.col + x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(width: u16, height: u16, rows: u16, store_w: u16) -> ClientSurface {
        ClientSurface { id: 1, width, height, rows, store_w, vmo_slot: 0, last_seq: 0 }
    }

    fn region(x: u32, row: u32, width: u32, height: u32) -> AtlasSurface {
        AtlasSurface { x, abs_row: row, width, height }
    }

    /// The base's rows land in the base's region, the store's in its own — wherever that is.
    #[test]
    fn base_rows_and_store_rows_land_in_their_own_regions() {
        let r = SurfaceRegions::of(
            &client(1440, 853, 2389, 1340),
            region(0, 6496, 1440, 853),
            Some(region(480, 9000, 1340, 1536)),
        );
        assert_eq!(r.rows, 2389);
        assert_eq!(r.place(0), Some((6496, 0, 1440)));
        assert_eq!(r.place(852), Some((6496 + 852, 0, 1440)));
        assert_eq!(r.place(853), Some((9000, 480, 1340)), "the store's first row");
        assert_eq!(r.place(2388), Some((9000 + 1535, 480, 1340)));
        assert_eq!(r.store_place(1080, 869), Some((9016, 1560)), "a band's pixel");
    }

    /// Nothing past what is placed: a store row without a store, a pixel past its width, a row
    /// past the surface.
    #[test]
    fn test_reject_a_place_outside_the_placed_regions() {
        let flat = SurfaceRegions::of(&client(1440, 853, 853, 0), region(0, 6496, 1440, 853), None);
        assert_eq!((flat.rows, flat.place(853)), (853, None));
        assert_eq!(flat.store_place(0, 900), None);
        let r = SurfaceRegions::of(
            &client(1440, 853, 2389, 1340),
            region(0, 6496, 1440, 853),
            Some(region(480, 9000, 1340, 1536)),
        );
        assert_eq!(r.place(2389), None);
        assert_eq!(r.store_place(1340, 900), None, "past the store's width");
        assert_eq!(r.store_place(0, 852), None, "a base row");
        // A store region shorter than the store (a client that grew its rows): cut to it.
        let short = SurfaceRegions::of(
            &client(1440, 853, 2389, 1340),
            region(0, 6496, 1440, 853),
            Some(region(480, 9000, 1340, 1000)),
        );
        assert_eq!((short.rows, short.place(1853)), (1853, None));
    }
}
