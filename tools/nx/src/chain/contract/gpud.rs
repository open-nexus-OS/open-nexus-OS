// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: GpudContract — simuliert den gpud-Dienst für Chain-Tests.
//! OWNERS: @tools-team
//!
//! Simuliert: virtio-gpu MMIO-Probe, Resource-Creation, Scanout, Cursor.

use crate::chain::contract::{Contract, ContractError};
use crate::chain::{ServiceId, SimIpcBus};

const OP_SET_FRAMEBUFFER_VMO: u8 = 3;
const OP_SUBMIT_ANIMATION_FRAME: u8 = 1;
const STATUS_OK: u8 = 0;
#[allow(dead_code)]
const STATUS_DEVICE_ERROR: u8 = 2;

/// Simulierter gpud-Dienst.
///
/// Standard-Pfad: virtio-gpu probe → 1280×800 Resource erstellen → Scanout setzen.
/// Fallback-Pfad: Resource-Creation fails → 64×64 Proof-Resource → gpud: ready ohne Scanout.
pub struct GpudContract {
    id: Option<ServiceId>,
    /// Wenn true: simuliert fehlschlagende Resource-Creation.
    pub resource_fails: bool,
    /// Externe VMO von windowd erhalten?
    #[allow(dead_code)]
    fb_received: bool,
}

impl GpudContract {
    #[allow(dead_code)]
    pub fn probe_only() -> Self {
        Self { id: None, resource_fails: false, fb_received: false }
    }

    #[allow(dead_code)]
    pub fn failing_resource() -> Self {
        Self { id: None, resource_fails: true, fb_received: false }
    }
}

impl Contract for GpudContract {
    fn service_name(&self) -> &'static str {
        "gpud"
    }

    fn set_service_id(&mut self, id: ServiceId) {
        self.id = Some(id);
    }

    fn run(&mut self, bus: &mut SimIpcBus) -> Result<(), ContractError> {
        let id =
            self.id.ok_or_else(|| ContractError::new(ServiceId(0), "gpud: service id not set"))?;

        // 1. MMIO-Probe
        bus.emit_marker(id, "gpud: virtio-gpu probed");

        // 2. Resource-Creation
        if self.resource_fails {
            bus.emit_marker(id, "gpud: resource create cmd fail");
            bus.emit_marker(id, "gpud: mmio fault");
            // Fallback: 64×64 proof resource
            bus.emit_marker(id, "gpud: ready");
            return Ok(());
        }

        // 3. Scanout setzen
        bus.emit_marker(id, "gpud: scanout ok");
        bus.emit_marker(id, "gpud: scanout 1280x800 bgra8888");
        bus.emit_marker(id, "gpud: cursor on");
        bus.emit_marker(id, "gpud: display ready (w=1280, h=800)");
        bus.emit_marker(id, "gpud: ready");

        // 4. IPC-Loop: auf windowd's Framebuffer-VMO warten
        let windowd_id = bus.service_id("windowd");
        if let Some(win_id) = windowd_id {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while std::time::Instant::now() < deadline {
                if let Some(msg) = bus.recv(id) {
                    if msg.from == win_id && msg.op == OP_SET_FRAMEBUFFER_VMO {
                        if msg.cap.is_some() {
                            self.fb_received = true;
                            // Zero-copy: VMO als virtio-gpu Resource-Backing
                            bus.emit_marker(id, "gpud: fb handoff ok");
                        }
                        break;
                    }
                    // Animation-Submit?
                    if msg.op == OP_SUBMIT_ANIMATION_FRAME {
                        bus.send(
                            id,
                            win_id,
                            OP_SUBMIT_ANIMATION_FRAME | 0x80,
                            vec![STATUS_OK],
                            None,
                        );
                    }
                }
                std::thread::sleep(std::time::Duration::from_micros(500));
            }
        }

        Ok(())
    }
}
