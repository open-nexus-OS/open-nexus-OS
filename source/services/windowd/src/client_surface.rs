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
pub const MAX_SURFACE_H: u16 = 800;
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

/// The surface table. v1/R1: exactly one client surface (the probe app);
/// the array shape is ready for MAX_APP_SURFACES growth.
#[derive(Debug, Default)]
pub struct ClientSurfaces {
    surface: Option<ClientSurface>,
    next_id: u32,
}

impl ClientSurfaces {
    #[must_use]
    pub fn new() -> Self {
        Self { surface: None, next_id: 1 }
    }

    #[must_use]
    pub fn get(&self) -> Option<&ClientSurface> {
        self.surface.as_ref()
    }

    /// Validates and registers a surface. Returns the new surface id or a
    /// wire status code.
    pub fn create(&mut self, width: u16, height: u16, format: u8, vmo_slot: u32) -> Result<u32, u8> {
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
        if self.surface.is_some() {
            return Err(SURFACE_STATUS_QUOTA);
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.surface = Some(ClientSurface { id, width, height, vmo_slot, last_seq: 0 });
        Ok(id)
    }

    /// Validates a present: known surface, strictly increasing seq (exactly
    /// one in flight — the app waits for the ack). Returns the surface +
    /// damage clamped to its bounds (empty rects dropped).
    pub fn present(
        &mut self,
        surface_id: u32,
        seq: u32,
        damage: &[DamageRect],
    ) -> Result<(ClientSurface, [DamageRect; MAX_DAMAGE_RECTS], usize), u8> {
        let Some(surface) = self.surface.as_mut() else {
            return Err(SURFACE_STATUS_BAD_SURFACE);
        };
        if surface.id != surface_id {
            return Err(SURFACE_STATUS_BAD_SURFACE);
        }
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
        match self.surface {
            Some(surface) if surface.id == surface_id => {
                self.surface = None;
                Ok(surface.vmo_slot)
            }
            _ => Err(SURFACE_STATUS_BAD_SURFACE),
        }
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
        // R1: one surface — the second create is refused, not silently replaced.
        assert_eq!(t.create(320, 240, FORMAT_BGRA8888, 11), Err(SURFACE_STATUS_QUOTA));
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
                    rect(90, 40, 50, 50),  // overhangs → clamped to 10x10
                    rect(200, 0, 5, 5),    // fully outside → dropped
                    rect(0, 0, 0, 10),     // empty → dropped
                    rect(0, 0, 100, 50),   // exact fit
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
