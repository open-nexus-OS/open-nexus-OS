// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Chain-Test: HidrawdContract — simulated hidrawd for IPC chain tests.
//! OWNERS: @tools-team
//!
//! Simulates hidrawd: polls virtio-input devices for raw HID events, normalizes
//! them into a wire batch, and sends it to inputd. Emits only the real markers
//! observed from the hidrawd service (source/services/hidrawd/src/os_lite.rs),
//! including the input-chain hops I1 (device event) and I2 (wire sent).

use crate::chain::contract::{Contract, ContractError};
use crate::chain::{ServiceId, SimIpcBus};

pub struct HidrawdContract {
    /// Whether a device produced input (raw HID seen → normalized → wire sent).
    pub has_input: bool,
    id: Option<ServiceId>,
}

impl HidrawdContract {
    #[allow(dead_code)]
    pub fn with_input() -> Self {
        Self { has_input: true, id: None }
    }

    #[allow(dead_code)]
    pub fn no_input() -> Self {
        Self { has_input: false, id: None }
    }
}

impl Contract for HidrawdContract {
    fn service_name(&self) -> &'static str {
        "hidrawd"
    }

    fn set_service_id(&mut self, id: ServiceId) {
        self.id = Some(id);
    }

    fn run(&mut self, bus: &mut SimIpcBus) -> Result<(), ContractError> {
        let id = self.id.ok_or_else(|| ContractError::new(ServiceId(0), "hidrawd: id not set"))?;

        bus.emit_marker(id, "hidrawd: os service payload ready");
        bus.emit_marker(id, "hidrawd: ready");

        if self.has_input {
            // Input-chain hops, string-identical to os_lite.rs. A real run shows
            // the last hop reached; this spec pins the order I1 → I2.
            bus.emit_marker(id, "hidrawd: chain I1 device event (raw HID polled)");
            bus.emit_marker(id, "hidrawd: ingress adapter ready");
            bus.emit_marker(id, "hidrawd: chain I2 wire sent to inputd");
        }

        Ok(())
    }
}
