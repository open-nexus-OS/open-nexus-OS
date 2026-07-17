// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ADR-0042 client-surface bookkeeping — the pure, host-tested state
//! machine behind windowd's cross-process surface transport: surface table
//! (R1: one slot), create validation (format/bounds/quota), strict seq/ack
//! flow control (one un-acked present in flight), damage clamping. The OS
//! blit (vmo_read → atlas rows) lives in the compositor runtime; this module
//! decides, the runtime moves pixels.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080D R1)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 6 tests
//! ADR: docs/adr/0042-cross-process-surface-transport.md

use nexus_display_proto::client_surface::{
    DamageRect, FORMAT_BGRA8888, MAX_DAMAGE_RECTS, SURFACE_STATUS_BAD_SEQ,
    SURFACE_STATUS_BAD_SURFACE, SURFACE_STATUS_MALFORMED, SURFACE_STATUS_QUOTA,
};

/// Bounds for app surfaces (ADR-0037's MAX_APP_SURFACES caps the count when the
/// table grows past one). Sized to the display so an app can go TRUE fullscreen
/// (the "□" toggle re-creates its surface at display size — see
/// `wm::toggle_fullscreen`). This is only a validation ceiling: the atlas band is
/// allocated at the CONTENT size (`app_window::open_app_window`, after the frame
/// is content-sized), and fullscreen skips the cached-blur band, so the ceiling
/// does NOT reserve display-sized rows per window.
pub const MAX_SURFACE_W: u16 = 1280;
// Raised for WebRender-style compositor scroll: a scrollable app uploads its
// FULL resident content as one tall atlas band (bounded by the app's own resident
// window, e.g. chat's tail(messages,64)), and gpud shifts only src_row per scroll
// frame. Non-scrollable surfaces (content_h == 0) still stay well under the old 800.
pub const MAX_SURFACE_H: u16 = 3072;
pub const MIN_SURFACE_DIM: u16 = 16;

/// One live client surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientSurface {
    pub id: u32,
    pub width: u16,
    pub height: u16,
    /// The app's surface VMO capability slot in windowd's table (moved in
    /// with `SURFACE_CREATE`).
    pub vmo_slot: u32,
    /// Last acked present sequence number (0 = none yet).
    pub last_seq: u32,
}

/// Maximum concurrently-resident client surfaces. Bounded (each surface owns a
/// VMO cap + an atlas band): enough for the desktop shell + greeter + a handful
/// of app windows coexisting. The single-`Option` era (R1: exactly one probe
/// app) is retired — the desktop-shell / greeter / app-window app-hosts each
/// own a surface (RFC-0065 multi-window). Callers address surfaces by id.
pub const MAX_APP_SURFACES: usize = 8;

/// The surface table: up to [`MAX_APP_SURFACES`] live client surfaces, addressed
/// by id. Each app-host (shell, greeter, an app) owns one; the compositor
/// composes them per window/z-role. Ids are monotonic (never reused) so a stale
/// id from a destroyed surface can never alias a new one.
#[derive(Debug)]
pub struct ClientSurfaces {
    surfaces: [Option<ClientSurface>; MAX_APP_SURFACES],
    next_id: u32,
}

impl Default for ClientSurfaces {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientSurfaces {
    #[must_use]
    pub fn new() -> Self {
        Self { surfaces: [None; MAX_APP_SURFACES], next_id: 1 }
    }

    /// The first live surface, if any. Back-compat accessor for the
    /// single-surface render path (retired as the runtime models N windows);
    /// new code addresses surfaces explicitly with [`Self::get_by_id`].
    #[must_use]
    pub fn get(&self) -> Option<&ClientSurface> {
        self.surfaces.iter().flatten().next()
    }

    /// The surface with `id`, if resident.
    #[must_use]
    pub fn get_by_id(&self, id: u32) -> Option<&ClientSurface> {
        self.surfaces.iter().flatten().find(|s| s.id == id)
    }

    /// Number of resident surfaces.
    #[must_use]
    pub fn count(&self) -> usize {
        self.surfaces.iter().flatten().count()
    }

    /// Iterate the resident surfaces (compositor walks these to compose windows).
    pub fn iter(&self) -> impl Iterator<Item = &ClientSurface> {
        self.surfaces.iter().flatten()
    }

    /// Validates and registers a surface in a free slot. Returns the new surface
    /// id, or a wire status code (`MALFORMED` on bad format/bounds, `QUOTA` when
    /// all [`MAX_APP_SURFACES`] slots are full).
    pub fn create(
        &mut self,
        width: u16,
        height: u16,
        format: u8,
        vmo_slot: u32,
    ) -> Result<u32, u8> {
        if format != FORMAT_BGRA8888 {
            return Err(SURFACE_STATUS_MALFORMED);
        }
        if width < MIN_SURFACE_DIM
            || height < MIN_SURFACE_DIM
            || width > MAX_SURFACE_W
            || height > MAX_SURFACE_H
        {
            return Err(SURFACE_STATUS_MALFORMED);
        }
        let Some(slot) = self.surfaces.iter_mut().find(|s| s.is_none()) else {
            return Err(SURFACE_STATUS_QUOTA);
        };
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        *slot = Some(ClientSurface { id, width, height, vmo_slot, last_seq: 0 });
        Ok(id)
    }

    /// Validates a present: known surface, strictly increasing seq (exactly
    /// one in flight per surface — the app waits for the ack). Returns the
    /// surface + damage clamped to its bounds (empty rects dropped).
    pub fn present(
        &mut self,
        surface_id: u32,
        seq: u32,
        damage: &[DamageRect],
    ) -> Result<(ClientSurface, [DamageRect; MAX_DAMAGE_RECTS], usize), u8> {
        let Some(surface) = self.surfaces.iter_mut().flatten().find(|s| s.id == surface_id) else {
            return Err(SURFACE_STATUS_BAD_SURFACE);
        };
        if seq != surface.last_seq.wrapping_add(1) {
            return Err(SURFACE_STATUS_BAD_SEQ);
        }
        let mut clamped = [DamageRect { x: 0, y: 0, width: 0, height: 0 }; MAX_DAMAGE_RECTS];
        let mut count = 0usize;
        for rect in damage.iter().take(MAX_DAMAGE_RECTS) {
            if rect.x >= surface.width || rect.y >= surface.height {
                continue;
            }
            let w = rect.width.min(surface.width - rect.x);
            let h = rect.height.min(surface.height - rect.y);
            if w == 0 || h == 0 {
                continue;
            }
            clamped[count] = DamageRect { x: rect.x, y: rect.y, width: w, height: h };
            count += 1;
        }
        surface.last_seq = seq;
        Ok((*surface, clamped, count))
    }

    /// Removes the surface; returns its VMO slot for the runtime to release.
    pub fn destroy(&mut self, surface_id: u32) -> Result<u32, u8> {
        let Some(slot) = self.surfaces.iter_mut().find(|s| s.is_some_and(|c| c.id == surface_id))
        else {
            return Err(SURFACE_STATUS_BAD_SURFACE);
        };
        let vmo_slot = slot.expect("slot matched Some above").vmo_slot;
        *slot = None;
        Ok(vmo_slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: u16, y: u16, w: u16, h: u16) -> DamageRect {
        DamageRect { x, y, width: w, height: h }
    }

    #[test]
    fn create_validates_format_bounds_and_quota() {
        let mut t = ClientSurfaces::new();
        assert_eq!(t.create(320, 240, 9, 10), Err(SURFACE_STATUS_MALFORMED));
        assert_eq!(t.create(8, 240, FORMAT_BGRA8888, 10), Err(SURFACE_STATUS_MALFORMED));
        assert_eq!(t.create(4096, 240, FORMAT_BGRA8888, 10), Err(SURFACE_STATUS_MALFORMED));
        let id = t.create(320, 240, FORMAT_BGRA8888, 10).expect("creates");
        assert_eq!(id, 1);
        // Multi-surface: further creates succeed until all slots are full, then
        // QUOTA (not silently replaced). One is already used, so MAX-1 more fit.
        for _ in 0..(MAX_APP_SURFACES - 1) {
            assert!(t.create(320, 240, FORMAT_BGRA8888, 11).is_ok());
        }
        assert_eq!(t.count(), MAX_APP_SURFACES);
        assert_eq!(t.create(320, 240, FORMAT_BGRA8888, 12), Err(SURFACE_STATUS_QUOTA));
    }

    #[test]
    fn multiple_surfaces_coexist_with_independent_seq_and_ids() {
        let mut t = ClientSurfaces::new();
        let a = t.create(320, 240, FORMAT_BGRA8888, 10).expect("a");
        let b = t.create(200, 100, FORMAT_BGRA8888, 11).expect("b");
        assert_ne!(a, b);
        assert_eq!(t.count(), 2);
        // Independent per-surface seq: advancing a does not affect b.
        assert!(t.present(a, 1, &[]).is_ok());
        assert!(t.present(b, 1, &[]).is_ok());
        assert!(t.present(a, 2, &[]).is_ok());
        assert_eq!(t.present(b, 3, &[]).unwrap_err(), SURFACE_STATUS_BAD_SEQ);
        // Destroy a → b survives; a's id is never reused (monotonic).
        assert_eq!(t.destroy(a), Ok(10));
        assert!(t.get_by_id(a).is_none());
        assert!(t.get_by_id(b).is_some());
        let c = t.create(64, 64, FORMAT_BGRA8888, 12).expect("c");
        assert_ne!(c, a);
        assert_ne!(c, b);
    }

    #[test]
    fn present_enforces_strict_seq() {
        let mut t = ClientSurfaces::new();
        let id = t.create(320, 240, FORMAT_BGRA8888, 10).expect("creates");
        assert_eq!(t.present(id, 2, &[]).unwrap_err(), SURFACE_STATUS_BAD_SEQ);
        assert!(t.present(id, 1, &[]).is_ok());
        // Replay of the same seq is refused (one in flight, acked in order).
        assert_eq!(t.present(id, 1, &[]).unwrap_err(), SURFACE_STATUS_BAD_SEQ);
        assert!(t.present(id, 2, &[]).is_ok());
    }

    #[test]
    fn present_rejects_unknown_surfaces() {
        let mut t = ClientSurfaces::new();
        assert_eq!(t.present(1, 1, &[]).unwrap_err(), SURFACE_STATUS_BAD_SURFACE);
        let id = t.create(320, 240, FORMAT_BGRA8888, 10).expect("creates");
        assert_eq!(t.present(id + 1, 1, &[]).unwrap_err(), SURFACE_STATUS_BAD_SURFACE);
    }

    #[test]
    fn damage_is_clamped_to_surface_bounds() {
        let mut t = ClientSurfaces::new();
        let id = t.create(100, 50, FORMAT_BGRA8888, 10).expect("creates");
        let (_, rects, count) = t
            .present(
                id,
                1,
                &[
                    rect(90, 40, 50, 50), // overhangs → clamped to 10x10
                    rect(200, 0, 5, 5),   // fully outside → dropped
                    rect(0, 0, 0, 10),    // empty → dropped
                    rect(0, 0, 100, 50),  // exact fit
                ],
            )
            .expect("presents");
        assert_eq!(count, 2);
        assert_eq!(rects[0], rect(90, 40, 10, 10));
        assert_eq!(rects[1], rect(0, 0, 100, 50));
    }

    #[test]
    fn destroy_releases_and_unknown_destroy_errors() {
        let mut t = ClientSurfaces::new();
        let id = t.create(320, 240, FORMAT_BGRA8888, 42).expect("creates");
        assert_eq!(t.destroy(id + 1).unwrap_err(), SURFACE_STATUS_BAD_SURFACE);
        assert_eq!(t.destroy(id), Ok(42));
        assert!(t.get().is_none());
        assert_eq!(t.destroy(id).unwrap_err(), SURFACE_STATUS_BAD_SURFACE);
    }

    #[test]
    fn ids_grow_across_lifecycles() {
        let mut t = ClientSurfaces::new();
        let a = t.create(320, 240, FORMAT_BGRA8888, 10).expect("creates");
        assert_eq!(t.destroy(a), Ok(10));
        let b = t.create(320, 240, FORMAT_BGRA8888, 11).expect("creates");
        assert_ne!(a, b, "ids are not recycled");
    }
}
