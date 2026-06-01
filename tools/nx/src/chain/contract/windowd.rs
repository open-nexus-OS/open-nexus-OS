// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: WindowdContract — simuliert den windowd-Dienst für Chain-Tests.
//! OWNERS: @tools-team
//!
//! Simuliert: Runtime-Initialisierung, Framebuffer-Registrierung,
//! First-Frame-Komposition.

use crate::chain::contract::{Contract, ContractError};
use crate::chain::{ServiceId, SimIpcBus};

const OP_SEND_COMPOSED_FRAME_VMO: u8 = 0x10;
const STATUS_OK: u8 = 0;
const STATUS_MALFORMED: u8 = 1;

/// Simulierter windowd-Dienst.
pub struct WindowdContract {
    id: Option<ServiceId>,
    width: u32,
    height: u32,
    hz: u16,
    wallpaper: bool,
}

impl WindowdContract {
    #[allow(dead_code)]
    pub fn visible_bootstrap(width: u32, height: u32) -> Self {
        Self { id: None, width, height, hz: 120, wallpaper: true }
    }

    #[allow(dead_code)]
    pub fn headless() -> Self {
        Self { id: None, width: 1280, height: 800, hz: 60, wallpaper: false }
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

        // 2. Auf Framebuffer-VMO von fbdevd warten
        let fbdevd_id = bus.service_id("fbdevd");
        let mut fb_registered = false;

        if let Some(fbdevd) = fbdevd_id {
            // Warte auf OP_SEND_COMPOSED_FRAME_VMO von fbdevd
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while std::time::Instant::now() < deadline {
                if let Some(msg) = bus.recv(id) {
                    if msg.from == fbdevd && msg.op == OP_SEND_COMPOSED_FRAME_VMO {
                        // VMO registrieren
                        if let Some(_cap) = msg.cap {
                            bus.emit_marker(id, "windowd: fb registered");
                            fb_registered = true;

                            // Antwort an fbdevd
                            bus.send(
                                id,
                                fbdevd,
                                OP_SEND_COMPOSED_FRAME_VMO | 0x80,
                                vec![STATUS_OK],
                                None,
                            );
                        } else {
                            bus.send(
                                id,
                                fbdevd,
                                OP_SEND_COMPOSED_FRAME_VMO | 0x80,
                                vec![STATUS_MALFORMED],
                                None,
                            );
                        }
                        break;
                    }
                }
                std::thread::sleep(std::time::Duration::from_micros(500));
            }
        }

        if fb_registered {
            // 3. Ersten Frame komponieren
            bus.emit_marker(id, "display: bootstrap on");
            bus.emit_marker(id, &format!("display: mode {}x{} argb8888", self.width, self.height));
            bus.emit_marker(id, "windowd: compose ready");
            bus.emit_marker(id, "windowd: present queued");
            bus.emit_marker(id, "windowd: present scheduler on");
            bus.emit_marker(id, "display: first scanout ok");
            bus.emit_marker(id, "systemui: first frame visible");
            bus.emit_marker(id, "windowd: present ok (seq=1 dmg=1)");
            bus.emit_marker(id, "SELFTEST: ui visible present ok");
        }

        Ok(())
    }
}
