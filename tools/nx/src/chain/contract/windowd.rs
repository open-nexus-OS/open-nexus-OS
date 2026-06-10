// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: WindowdContract — simulated windowd service for Chain-Tests.
//! OWNERS: @tools-team
//!
//! GPU-only architecture (RFC-0059 Phase 6): windowd is the sole display owner.
//! It creates its own framebuffer VMO and hands it off to gpud for scanout.
//! No fbdevd, no ramfb — one owner, one path.

use crate::chain::contract::{Contract, ContractError};
use crate::chain::{ServiceId, SimIpcBus};

/// Simulierter windowd-Dienst (GPU-only self-bootstrap).
pub struct WindowdContract {
    id: Option<ServiceId>,
    width: u32,
    height: u32,
    hz: u16,
    #[allow(dead_code)]
    wallpaper: bool,
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
        bus.emit_marker(id, "windowd: runtime init ok");
        bus.emit_marker(
            id,
            &format!("windowd: ready (w={}, h={}, hz={})", self.width, self.height, self.hz),
        );

        // 2. GPU self-bootstrap: windowd creates own framebuffer VMO
        bus.emit_marker(id, "windowd: backend=gpu");

        // 3. Bootsplash/Display start
        bus.emit_marker(id, "display: bootstrap on");
        bus.emit_marker(id, &format!("display: mode {}x{} argb8888", self.width, self.height));
        bus.emit_marker(id, "windowd: backend=visible");
        bus.emit_marker(id, "windowd: compose ready");
        bus.emit_marker(id, "windowd: present ok (seq=1 dmg=1)");
        bus.emit_marker(id, "windowd: present visible ok");
        bus.emit_marker(id, "windowd: present scheduler on");
        bus.emit_marker(id, "display: first scanout ok");
        bus.emit_marker(id, "systemui: first frame visible");
        bus.emit_marker(id, "SELFTEST: ui v2 present ok");
        bus.emit_marker(id, "SELFTEST: ui visible present ok");
        bus.emit_marker(id, "windowd: input visible on");
        bus.emit_marker(id, "windowd: cursor move visible");
        bus.emit_marker(id, "windowd: hover visible");
        bus.emit_marker(id, "windowd: focus visible");
        bus.emit_marker(id, "launcher: click visible ok");
        bus.emit_marker(id, "windowd: keyboard visible");
        bus.emit_marker(id, "windowd: wheel visible");
        bus.emit_marker(id, "SELFTEST: ui visible input ok");
        bus.emit_marker(id, "SELFTEST: ui visible wheel ok");
        bus.emit_marker(id, "uiruntime: on");
        bus.emit_marker(id, "windowd: implicit transitions on");
        bus.emit_marker(id, "uianim: timeline on");
        bus.emit_marker(id, "uiruntime: batch commit ok");
        bus.emit_marker(id, "windowd: live transition ok");
        bus.emit_marker(id, "uianim: spring converge ok");
        bus.emit_marker(id, "SELFTEST: ui v5 transition ok");

        // 4. Phase 1-8: GPU-first display pipeline (reactive, no polling)
        if self.gpud_available {
            bus.emit_marker(id, "windowd: fb vmo create ok");
            bus.emit_marker(id, "windowd: handoff attach sent");
            bus.emit_marker(id, "windowd: handoff attach ack");
            bus.emit_marker(id, "windowd: handoff present sent");
            bus.emit_marker(id, "windowd: present visible ok");
            // Phase 2: GPU blur present
            bus.emit_marker(id, "windowd: present frame with GPU blur");
            // Phase 7: pacer-driven frame
            bus.emit_marker(id, "windowd: pacer frame ok");
            // Phase 6: cursor move + hover (v3b markers)
            bus.emit_marker(id, "windowd: cursor move visible");
            bus.emit_marker(id, "windowd: hover visible");
            // Observer confirms
            bus.emit_marker(id, "SELFTEST: ui visible present ok");
            bus.emit_marker(id, "SELFTEST: ui visible input ok");
        }

        Ok(())
    }
}
