// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Narrow `inputd` route target seam preserving `windowd` as authority.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 8 host contract tests in the `inputd` crate.
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

pub trait RouteTarget {
    fn route_pointer_move(&mut self, x: i32, y: i32) -> windowd::Result<windowd::InputDelivery>;
    fn route_pointer_down(&mut self, x: i32, y: i32) -> windowd::Result<windowd::InputDelivery>;
    fn route_keyboard(&mut self, key_code: u32) -> windowd::Result<windowd::InputDelivery>;
    fn route_touch(
        &mut self,
        x: i32,
        y: i32,
        phase: windowd::TouchInputPhase,
    ) -> windowd::Result<windowd::InputDelivery>;
    fn bounds(&self) -> (u32, u32);

    /// Attempt to coalesce a pointer-move event. Default falls through to route_pointer_move.
    fn try_coalesce_pointer_move(
        &mut self,
        x: i32,
        y: i32,
    ) -> windowd::Result<windowd::InputDelivery> {
        self.route_pointer_move(x, y)
    }
}

impl RouteTarget for windowd::WindowServer {
    fn route_pointer_move(&mut self, x: i32, y: i32) -> windowd::Result<windowd::InputDelivery> {
        windowd::WindowServer::route_pointer_move(self, x, y)
    }

    fn route_pointer_down(&mut self, x: i32, y: i32) -> windowd::Result<windowd::InputDelivery> {
        windowd::WindowServer::route_pointer_down(self, x, y)
    }

    fn route_keyboard(&mut self, key_code: u32) -> windowd::Result<windowd::InputDelivery> {
        windowd::WindowServer::route_keyboard(self, key_code)
    }

    fn route_touch(
        &mut self,
        x: i32,
        y: i32,
        phase: windowd::TouchInputPhase,
    ) -> windowd::Result<windowd::InputDelivery> {
        windowd::WindowServer::route_touch(self, x, y, phase)
    }

    fn bounds(&self) -> (u32, u32) {
        let config = self.config();
        (config.width, config.height)
    }

    fn try_coalesce_pointer_move(
        &mut self,
        x: i32,
        y: i32,
    ) -> windowd::Result<windowd::InputDelivery> {
        match windowd::WindowServer::try_coalesce_pointer_move(self, x, y) {
            Ok(true) => {
                // Coalesced: return a synthetic delivery marking it as skipped
                let pos = self.pointer_position().unwrap_or(windowd::PointerPosition { x, y });
                Ok(windowd::InputDelivery {
                    seq: windowd::InputSeq::new(0),
                    surface: windowd::SurfaceId::new(0),
                    kind: windowd::InputEventKind::PointerMove { x: pos.x, y: pos.y },
                })
            }
            Ok(false) => self.route_pointer_move(x, y),
            Err(_) => self.route_pointer_move(x, y),
        }
    }
}

/// Production-grade input architecture (a pure input pipeline feeding a
/// compositor window-server): inputd is a **pure input pipeline** — it normalizes HID
/// (pointer transform, accel, coalescing, keymap) and forwards normalized events
/// to the compositor; it does **no hit-testing and owns no window server**. The
/// compositor (windowd, `interaction.rs` SSOT since A1) resolves which surface an
/// event hits. `NormalizeRouter` is that pure seam: it passes coordinates through
/// as the "delivery" (surface 0 = "let the compositor route it") and does only
/// distance-threshold pointer coalescing. It replaces the embedded
/// `windowd::WindowServer` that inputd used to carry — removing the duplicate
/// hit-testing path.
/// (Constructed by the os-lite runtime entry; host builds compile the type
/// for the shared `RouteTarget` seam only, hence the scoped allow.)
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub struct NormalizeRouter {
    width: u32,
    height: u32,
    seq: u64,
    /// Last delivered pointer position, for coalescing (squared-distance gate).
    last_pointer: Option<(i32, i32)>,
    /// Coalesce threshold in pixels (moves closer than this are dropped).
    coalesce_px: i32,
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
impl NormalizeRouter {
    pub fn new(width: u32, height: u32, coalesce_px: i32) -> Self {
        Self { width, height, seq: 0, last_pointer: None, coalesce_px }
    }

    fn next_seq(&mut self) -> windowd::InputSeq {
        self.seq = self.seq.wrapping_add(1);
        windowd::InputSeq::new(self.seq)
    }

    fn deliver(
        &mut self,
        kind: windowd::InputEventKind,
    ) -> windowd::Result<windowd::InputDelivery> {
        let seq = self.next_seq();
        // Surface 0 = unrouted: the compositor (windowd) performs hit-testing.
        Ok(windowd::InputDelivery { seq, surface: windowd::SurfaceId::new(0), kind })
    }

    /// Pure pipeline: there is no local surface to deliver into (windowd owns
    /// all surfaces). No-op for the embedded-WindowServer `drain_input_events`
    /// the OS-lite runtime used to call; returns 0 delivered (telemetry only).
    pub fn drain_input_events(
        &mut self,
        _launcher: windowd::CallerCtx,
        _surface: windowd::SurfaceId,
    ) -> windowd::Result<usize> {
        Ok(0)
    }

    /// Pure pipeline: inputd does NOT track focus — windowd (compositor) owns it.
    /// Report the unrouted surface (0) so the runtime's "deliver to the active
    /// surface" gates pass through; windowd resolves the real focus/hit-test.
    pub fn focused_surface(&self) -> Option<windowd::SurfaceId> {
        Some(windowd::SurfaceId::new(0))
    }
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
impl RouteTarget for NormalizeRouter {
    fn route_pointer_move(&mut self, x: i32, y: i32) -> windowd::Result<windowd::InputDelivery> {
        self.last_pointer = Some((x, y));
        self.deliver(windowd::InputEventKind::PointerMove { x, y })
    }

    fn route_pointer_down(&mut self, x: i32, y: i32) -> windowd::Result<windowd::InputDelivery> {
        self.last_pointer = Some((x, y));
        self.deliver(windowd::InputEventKind::PointerDown)
    }

    fn route_keyboard(&mut self, key_code: u32) -> windowd::Result<windowd::InputDelivery> {
        self.deliver(windowd::InputEventKind::Keyboard { key_code })
    }

    fn route_touch(
        &mut self,
        x: i32,
        y: i32,
        phase: windowd::TouchInputPhase,
    ) -> windowd::Result<windowd::InputDelivery> {
        let kind = match phase {
            windowd::TouchInputPhase::Down => windowd::InputEventKind::TouchDown { x, y },
            windowd::TouchInputPhase::Move => windowd::InputEventKind::TouchMove { x, y },
            windowd::TouchInputPhase::Up => windowd::InputEventKind::TouchUp { x, y },
        };
        self.deliver(kind)
    }

    fn bounds(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn try_coalesce_pointer_move(
        &mut self,
        x: i32,
        y: i32,
    ) -> windowd::Result<windowd::InputDelivery> {
        if let Some((lx, ly)) = self.last_pointer {
            let (dx, dy) = (x - lx, y - ly);
            if dx * dx + dy * dy < self.coalesce_px * self.coalesce_px {
                // Within threshold: coalesce — report the last position, no new route.
                return self.deliver(windowd::InputEventKind::PointerMove { x: lx, y: ly });
            }
        }
        self.route_pointer_move(x, y)
    }
}
