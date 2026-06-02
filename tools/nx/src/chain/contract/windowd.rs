// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: WindowdContract — simuliert den windowd-Dienst für Chain-Tests.
//! OWNERS: @tools-team
//!
//! GPU-only architecture (RFC-0059 Phase 6): windowd is the sole display owner.
//! It creates its own framebuffer VMO and hands it off to gpud for scanout.
//! No fbdevd, no ramfb — one owner, one path.

use crate::chain::contract::{Contract, ContractError};
use crate::chain::{ServiceId, SimIpcBus};

const STATUS_OK: u8 = 0;

/// Simulierter windowd-Dienst (GPU-only self-bootstrap).
pub struct WindowdContract {
    id: Option<ServiceId>,
    width: u32,
    height: u32,
    hz: u16,
    wallpaper: bool,
    /// Whether gpud is available for GPU scanout handoff.
    gpud_available: bool,
}

impl WindowdContract {
    #[allow(dead_code)]
    pub fn visible_bootstrap(width: u32, height: u32) -> Self {
        Self { id: None, width, height, hz: 120, wallpaper: true, gpud_available: true }
    }

    #[allow(dead_code)]
    pub fn headless() -> Self {
        Self { id: None, width: 1280, height: 800, hz: 60, wallpaper: false, gpud_available: false }
    }
}

impl Contract for WindowdContract {
    fn service_name(&self) -> &'static str {
        "windowd"
    }

    fn set_service_id(&mut self, id: ServiceId) {
        self.id = Some(id);
    }

    fn run(&mut self, bus: &mut SimIpcBus) -> Result<(), ContractError> {
        let id = self
            .id
            .ok_or_else(|| ContractError::new(ServiceId(0), "windowd: service id not set"))?;

        // 1. Runtime initialisieren
        bus.emit_marker(id, "windowd: runtime init start");

        // Wallpaper laden (simuliert)
        if self.wallpaper {
            bus.emit_marker(id, "windowd: wallpaper loaded (jpeg)");
        } else {
            bus.emit_marker(id, "windowd: wallpaper fallback solid");
        }

        bus.emit_marker(id, "windowd: runtime init ok");
        bus.emit_marker(
            id,
            &format!("windowd: ready (w={}, h={}, hz={})", self.width, self.height, self.hz),
        );

        // 2. GPU self-bootstrap: windowd creates own framebuffer VMO (vmo_create).
        // No handoff from another service — one owner, one path.
        bus.emit_marker(id, "windowd: backend=gpu");

        // 3. Ersten Frame komponieren und presenten
        bus.emit_marker(id, &format!("display: mode {}x{} argb8888", self.width, self.height));
        bus.emit_marker(id, "windowd: compose ready");
        bus.emit_marker(id, "windowd: backend=visible");
        bus.emit_marker(id, "windowd: present ok (seq=1 dmg=1)");
        bus.emit_marker(id, "windowd: present visible ok");
        bus.emit_marker(id, "windowd: present scheduler on");
        bus.emit_marker(id, "display: first scanout ok");
        bus.emit_marker(id, "systemui: first frame visible");

        // 4. Optional: gpud scanout handoff
        if self.gpud_available {
            if let Some(gpud_id) = bus.service_id("gpud") {
                bus.emit_marker(id, "gpud: virtio-gpu probed");
                bus.emit_marker(id, "gpud: scanout ok");
                bus.emit_marker(id, "gpud: cursor on");
                bus.emit_marker(id, "gpud: ready");
            }
        }

        bus.emit_marker(id, "SELFTEST: ui visible present ok");

        Ok(())
    }
}
