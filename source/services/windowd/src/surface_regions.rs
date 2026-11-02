// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: where a client surface's pixels are when a frame samples them (RFC-0109, by
//! reference since RFC-0114): in the surface's buffer, the slot of its newest present — rows
//! `[0, height)` its BASE, rows `[height, rows)` its STORE, of which columns `[0, store_w)` are
//! used, at the same rows in the slot (the app draws them where the GPU reads). The one mapping
//! the layer encoder samples by: a surface row or a store pixel to its place in the slot, nothing
//! past the surface's rows and the store's width; and what every layer names — the buffer
//! (`source`), the slot, the content's generation.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below

use crate::client_surface::{ClientSurface, Shown};

/// A surface's places in its buffer's slot, as its layers sample them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SurfaceRegions {
    /// The rows a layer may sample: the base's and the store's.
    rows: u32,
    /// The surface's base height: rows below it are the store's.
    height: u32,
    /// The base's width.
    width: u32,
    /// The store's used width (`0`: no store).
    store_w: u32,
    /// What the layers name: the surface's buffer, …
    pub(crate) source: u32,
    /// … the slot of its newest present, …
    pub(crate) slot: u8,
    /// … and that present's seq — the content's generation (a layer's `content_epoch`).
    pub(crate) generation: u32,
}

impl SurfaceRegions {
    /// The places of `client`'s newest frame (`shown`).
    pub(crate) fn of(client: &ClientSurface, shown: Shown) -> Self {
        Self {
            rows: u32::from(client.rows),
            height: u32::from(client.height),
            width: u32::from(client.width),
            store_w: u32::from(client.store_w),
            source: shown.buffer.source,
            slot: shown.slot,
            generation: shown.generation,
        }
    }

    /// Surface row `y` in the slot: its row, its first column and the columns used; `None` past
    /// the surface's rows.
    pub(crate) fn place(&self, y: u32) -> Option<(u32, u32, u32)> {
        if y < self.height {
            return Some((y, 0, self.width));
        }
        (y < self.rows && self.store_w > 0).then_some((y, 0, self.store_w))
    }

    /// The slot's place of store pixel `(x, y)` (surface coordinates, `y` at or below the
    /// base's height): its row and column — what a slice or a wash samples. `None` outside the
    /// store.
    pub(crate) fn store_place(&self, x: u32, y: u32) -> Option<(u32, u32)> {
        (y >= self.height && y < self.rows && x < self.store_w).then_some((y, x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_surface::{ClientSurfaces, SurfaceBuffer};
    use nexus_display_proto::client_surface::FORMAT_BGRA8888;

    /// A surface of `width × height` with `rows` rows and a store `store_w` wide, presented once
    /// into slot 1 of buffer 9.
    fn presented(width: u16, height: u16, rows: u16, store_w: u16) -> (ClientSurface, Shown) {
        let mut t = ClientSurfaces::new();
        let id = t.create(width, height, (rows, store_w), FORMAT_BGRA8888).expect("creates");
        let buffer = SurfaceBuffer {
            source: 9,
            stride: u32::from(width) * 4,
            width,
            rows,
            slots: 2,
            cache_block: 0,
        };
        t.bind_buffer(id, buffer, None).expect("binds");
        t.present(id, (1, 1), &[], 0).expect("presents");
        let s = *t.get_by_id(id).expect("kept");
        (s, s.shown().expect("shown"))
    }

    /// The base's rows and the store's are the slot's own rows; every layer names the buffer,
    /// the slot of the newest present and its generation.
    #[test]
    fn base_rows_and_store_rows_are_the_slots_own() {
        let (c, shown) = presented(1440, 853, 2389, 1340);
        let r = SurfaceRegions::of(&c, shown);
        assert_eq!((r.source, r.slot, r.generation), (9, 1, 1));
        assert_eq!(r.place(0), Some((0, 0, 1440)));
        assert_eq!(r.place(852), Some((852, 0, 1440)));
        assert_eq!(r.place(853), Some((853, 0, 1340)), "the store's first row");
        assert_eq!(r.place(2388), Some((2388, 0, 1340)));
        assert_eq!(r.store_place(1080, 869), Some((869, 1080)), "a band's pixel");
    }

    /// Nothing past the surface: a store row without a store, a pixel past its width, a row past
    /// the surface.
    #[test]
    fn test_reject_a_place_outside_the_surface() {
        let (flat, shown) = presented(1440, 853, 853, 0);
        let flat = SurfaceRegions::of(&flat, shown);
        assert_eq!(flat.place(853), None);
        assert_eq!(flat.store_place(0, 900), None);
        let (c, shown) = presented(1440, 853, 2389, 1340);
        let r = SurfaceRegions::of(&c, shown);
        assert_eq!(r.place(2389), None);
        assert_eq!(r.store_place(1340, 900), None, "past the store's width");
        assert_eq!(r.store_place(0, 852), None, "a base row");
    }
}
