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

/// How a frame must reach a client's event channel (TASK-0306).
///
/// The distinction is not "important vs unimportant" — it is **whether the
/// client is waiting**. Getting it wrong is invisible in the code and very
/// visible on screen: a dropped present-ack does not lose a pixel, it leaves
/// the client blocked on a reply that will never come, and the user sees a
/// frozen UI. That is the exact shape of the 2026-07-26 report.
///
/// `Blocking` is a REACTIVE park, not a retry loop. The kernel already
/// implements the rendezvous — a non-NONBLOCK `ipc_send_v1` registers the
/// sender as a send-waiter on the endpoint and the receive path pops it the
/// moment capacity appears. Polling with `yield_()` instead would issue
/// hundreds of syscalls of the very kind measured at up to 21 ms under lock
/// contention, and could still lose the frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Delivery {
    /// The client is (or may be) BLOCKED on this frame: present, create and
    /// destroy acks. Failure is real and must be reported as failure — but a
    /// RECOVERY PATH exists (`compositor/mod.rs` falls back to
    /// `reply_and_close_wait` / `server.send`, both `Wait::Blocking`), so the
    /// budget only has to cover the common case.
    Blocking,
    /// USER INTENT with no recovery path: a tap, or a settings push (region /
    /// locale / theme / shell profile). Nothing downstream reconstructs it and
    /// no fallback catches it — if this send expires the click is gone, or the
    /// window keeps the old language forever. It therefore gets a LIVENESS
    /// budget instead of a frame budget.
    Critical,
    /// The NEXT frame supersedes this one: hover motion. One attempt — a full
    /// queue means the client is behind, and the newer frame carries the newer
    /// truth anyway. Retrying would only add latency to data already stale.
    ///
    /// NOT for settings pushes. This doc used to list "region/theme pushes"
    /// here, which was wrong in the way that matters: hover motion regenerates
    /// dozens of times a second, a region push happens ONCE when the user picks
    /// a language and nothing ever repeats it. They are opposites, not
    /// neighbours. See [`Delivery::Critical`].
    Coalescing,
}

/// How long the compositor will wait for a blocked client to drain one ACK,
/// in nanoseconds (16 ms — one frame at 60 Hz).
///
/// This is a DEADLINE handed to the kernel, not a retry budget: `ipc_send_v1`
/// without `IPC_SYS_NONBLOCK` parks the sender on the endpoint
/// (`register_send_waiter`) and the receive path wakes it the moment the queue
/// drains (`pop_send_waiter`). One syscall, no polling.
///
/// One frame is enough HERE because expiry is not the end of the story: the
/// caller falls back to the blocking reply path, which cannot be lost.
pub(crate) const BLOCKING_SEND_DEADLINE_NS: u64 = 16_000_000;

/// How long the compositor will wait to hand a client a TAP (250 ms).
///
/// Sized from measurement, not from the refresh rate. The frame budget was the
/// wrong unit: under load the compositor loop was observed at **13 Hz** (77 ms
/// per iteration), so a 16 ms deadline expired on a merely-slow client and the
/// user's click vanished with a `FAIL desktop input send` — the exact symptom
/// this constant exists to prevent. 250 ms covers three of those worst-observed
/// iterations.
///
/// The cost of the larger bound is paid only when a client really is wedged,
/// and it is the right trade: one visible 250 ms hitch is recoverable, a
/// silently dropped click is not. The bound still exists — an unbounded send
/// would let one dead client wedge the compositor forever.
pub(crate) const CRITICAL_SEND_DEADLINE_NS: u64 = 250_000_000;

impl Delivery {
    /// Deadline in ns for a blocking send; `None` = do not block at all.
    pub(crate) const fn deadline_ns(self) -> Option<u64> {
        match self {
            Delivery::Blocking => Some(BLOCKING_SEND_DEADLINE_NS),
            Delivery::Critical => Some(CRITICAL_SEND_DEADLINE_NS),
            Delivery::Coalescing => None,
        }
    }

    /// The class an input kind belongs to. A tap is discrete — nothing
    /// downstream can reconstruct it. Hover motion is superseded by the next
    /// move, so retrying it only delivers stale coordinates late.
    pub(crate) const fn for_input(is_tap: bool) -> Self {
        if is_tap {
            Delivery::Critical
        } else {
            Delivery::Coalescing
        }
    }
}

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
    #[cfg_attr(not(test), allow(dead_code))] // documented back-compat accessor, exercised by host unit tests
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
    #[cfg_attr(not(test), allow(dead_code))] // registry vocabulary, exercised by host unit tests
    pub fn count(&self) -> usize {
        self.surfaces.iter().flatten().count()
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

#[cfg(test)]
mod delivery_tests {
    use super::{Delivery, BLOCKING_SEND_DEADLINE_NS, CRITICAL_SEND_DEADLINE_NS};

    /// TASK-0306: the bug was a POLICY bug, so the policy is what gets pinned.
    /// A frame the client is blocked on must be retried; a frame the next one
    /// supersedes must not be. Getting this backwards is invisible in review
    /// and shows up as a frozen UI — a present-ack was fired once, dropped on
    /// a full queue, and the client waited 504 ms for a reply that never came.
    #[test]
    fn blocking_retries_and_coalescing_does_not() {
        assert_eq!(Delivery::Coalescing.deadline_ns(), None, "stale data must not block");
        assert_eq!(
            Delivery::Blocking.deadline_ns(),
            Some(BLOCKING_SEND_DEADLINE_NS),
            "a client is waiting on this frame — park on the endpoint, do not poll"
        );
        assert!(
            BLOCKING_SEND_DEADLINE_NS <= 16_000_000,
            "an ACK has a fallback path, so one frame is the right bound"
        );
    }

    /// The follow-up bug: sizing the TAP budget to a frame was wrong, because
    /// nothing catches an expired tap. The compositor loop was measured at
    /// 13 Hz under load (77 ms), so a 16 ms budget dropped real clicks with
    /// `FAIL desktop input send`. User intent outlives one frame.
    #[test]
    fn a_tap_outlives_a_slow_frame() {
        const WORST_OBSERVED_LOOP_NS: u64 = 77_000_000; // 13 Hz, from the boot log
        assert!(
            CRITICAL_SEND_DEADLINE_NS > WORST_OBSERVED_LOOP_NS,
            "a tap must survive the slowest loop iteration we have actually measured"
        );
        assert!(
            CRITICAL_SEND_DEADLINE_NS > BLOCKING_SEND_DEADLINE_NS,
            "user intent has no recovery path; an ack does"
        );
        assert!(
            CRITICAL_SEND_DEADLINE_NS <= 500_000_000,
            "still bounded — one dead client must not wedge the compositor forever"
        );
    }

    /// A settings push (locale/theme/profile) is NOT hover motion. Hover
    /// regenerates dozens of times a second; a locale push happens once, when
    /// the user picks a language, and nothing repeats it. Filing them in the
    /// same class is what left a window stuck in the old language after a
    /// momentarily full queue — silently, since the result was discarded.
    #[test]
    fn a_settings_push_is_not_coalescable() {
        assert_eq!(
            Delivery::Critical.deadline_ns(),
            Some(CRITICAL_SEND_DEADLINE_NS),
            "settings pushes share the tap's class: user intent, no recovery path"
        );
        assert_ne!(
            Delivery::Critical.deadline_ns(),
            Delivery::Coalescing.deadline_ns(),
            "a dropped settings push is never superseded by a later frame"
        );
    }

    #[test]
    fn taps_block_hover_coalesces() {
        assert_eq!(
            Delivery::for_input(true),
            Delivery::Critical,
            "a tap is discrete AND unrecoverable"
        );
        assert_eq!(
            Delivery::for_input(false),
            Delivery::Coalescing,
            "the next motion supersedes this one"
        );
    }
}
