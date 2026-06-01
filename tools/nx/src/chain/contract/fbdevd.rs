// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: FbdevdContract — simuliert den fbdevd-Dienst für Chain-Tests.
//! OWNERS: @tools-team
//!
//! Simuliert den Boot-Pfad: Framebuffer-VMO allozieren, ramfb konfigurieren,
//! Splash rendern, VMO an windowd senden.

use crate::chain::contract::{Contract, ContractError, SimCapDesc, SimCapHandle};
use crate::chain::{ServiceId, SimIpcBus};

/// Konstante für OP_SEND_COMPOSED_FRAME_VMO (muss mit `fbdevd`-Protokoll übereinstimmen).
const OP_SEND_COMPOSED_FRAME_VMO: u8 = 0x10;
const STATUS_OK: u8 = 0;

/// Simulierter fbdevd-Dienst.
///
/// Emittiert Marker:
/// - `fbdevd: ready` — nach Framebuffer-Allokation
/// - `fbdevd: map ok` — nach erfolgreichem VMO-Mapping
/// - `fbdevd: ramfb configured` — nach simulierter ramfb-Konfiguration
/// - `fbdevd: flush ok` — nach erfolgreicher VMO-Übergabe an windowd
pub struct FbdevdContract {
    id: Option<ServiceId>,
    width: u32,
    height: u32,
    splash: bool,
    #[allow(dead_code)]
    fb_handle: Option<SimCapHandle>,
}

impl FbdevdContract {
    /// Erzeugt einen fbdevd-Contract mit den gegebenen Display-Dimensionen.
    #[allow(dead_code)]
    pub fn new(width: u32, height: u32, splash: bool) -> Self {
        Self { id: None, width, height, splash, fb_handle: None }
    }
}

impl Contract for FbdevdContract {
    fn service_name(&self) -> &'static str {
        "fbdevd"
    }

    fn initial_caps(&self) -> Vec<SimCapDesc> {
        vec![SimCapDesc::Endpoint { target: "windowd".into() }]
    }

    fn set_service_id(&mut self, id: ServiceId) {
        self.id = Some(id);
    }

    fn run(&mut self, bus: &mut SimIpcBus) -> Result<(), ContractError> {
        let id = self
            .id
            .ok_or_else(|| ContractError::new(ServiceId(0), "fbdevd: service id not set"))?;

        // 1. Framebuffer-VMO allozieren
        let byte_len = (self.width * self.height * 4) as usize;
        let fb = bus.alloc_vmo(id, byte_len);
        self.fb_handle = Some(fb);
        bus.emit_marker(id, "fbdevd: map ok");

        // 2. Ramfb konfigurieren (simuliert — im echten System via fw_cfg DMA)
        bus.emit_marker(id, "fbdevd: ramfb configured");

        // 3. Splash rendern (optional)
        if self.splash {
            // Im echten System: vmo_write in 800 Zeilen
            // Hier: simuliert durch Marker
        }
        bus.emit_marker(id, "fbdevd: ready");

        // 4. Framebuffer-VMO an windowd senden
        let windowd_id = bus
            .service_id("windowd")
            .ok_or_else(|| ContractError::new(id, "fbdevd: windowd not registered"))?;

        let fb_clone =
            bus.cap_clone(fb).ok_or_else(|| ContractError::new(id, "fbdevd: cap_clone failed"))?;

        let request = [OP_SEND_COMPOSED_FRAME_VMO, 0, 0, 0];
        bus.send(id, windowd_id, OP_SEND_COMPOSED_FRAME_VMO, request.to_vec(), Some(fb_clone));

        // 5. Auf Antwort von windowd warten (polling)
        let mut got_reply = false;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while std::time::Instant::now() < deadline {
            if let Some(msg) = bus.recv(id) {
                if msg.from == windowd_id && msg.op == OP_SEND_COMPOSED_FRAME_VMO | 0x80 {
                    if msg.payload.first() == Some(&STATUS_OK) {
                        got_reply = true;
                        bus.emit_marker(id, "fbdevd: flush ok");
                    }
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_micros(500));
        }

        if !got_reply {
            bus.emit_marker(id, "fbdevd: flush fail");
        }

        Ok(())
    }
}
