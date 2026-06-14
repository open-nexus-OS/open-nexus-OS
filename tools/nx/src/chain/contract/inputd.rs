// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Chain-Test: InputdContract — simulated inputd for IPC chain tests.
//! OWNERS: @tools-team
//!
//! Simulates inputd's behavior: receives HID events, runs pointer-accel,
//! pushes VisibleState to windowd via IPC (priority-wired slots 5/6).

use crate::chain::contract::{Contract, ContractError};
use crate::chain::{ServiceId, SimIpcBus};

pub struct InputdContract {
    /// Whether to push cursor move events.
    pub cursor_moves: bool,
    id: Option<ServiceId>,
}

impl InputdContract {
    #[allow(dead_code)]
    pub fn with_cursor_moves() -> Self {
        Self { cursor_moves: true, id: None }
    }

    #[allow(dead_code)]
    pub fn no_input() -> Self {
        Self { cursor_moves: false, id: None }
    }
}

impl Contract for InputdContract {
    fn service_name(&self) -> &'static str {
        "inputd"
    }

    fn set_service_id(&mut self, id: ServiceId) {
        self.id = Some(id);
    }

    fn run(&mut self, bus: &mut SimIpcBus) -> Result<(), ContractError> {
        let id = self.id.ok_or_else(|| ContractError::new(ServiceId(0), "inputd: id not set"))?;

        bus.emit_marker(id, "inputd: starting");
        // Priority-wired: inputd uses init-assigned slots (5=send, 6=recv),
        // bypassing the kernel route table for deterministic IPC.
        bus.emit_marker(id, "inputd: priority-wired slots 5/6 ok");

        if self.cursor_moves {
            // Input-chain hops I3..I5, string-identical to os_lite.rs.
            bus.emit_marker(id, "inputd: chain I3 wire recv from hidrawd");
            bus.emit_marker(id, "inputd: chain I4 normalized");
            bus.emit_marker(id, "inputd: cursor move computed");
            bus.emit_marker(id, "inputd: windowd visible-state pushed");
            bus.emit_marker(id, "inputd: chain I5 delivered to windowd");
        }

        bus.emit_marker(id, "inputd: ready");
        Ok(())
    }
}
