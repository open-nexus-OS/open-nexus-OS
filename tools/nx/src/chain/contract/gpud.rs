// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: GpudContract — simulated gpud service for Chain-Tests.
//! OWNERS: @tools-team
//!
//! Simulated gpud service.
//!
//! Emits only the real markers observed from the gpud service:
//!   gpud: virtio-gpu probed → gpud: ready

use crate::chain::contract::{Contract, ContractError};
use crate::chain::{ServiceId, SimIpcBus};

/// Simulated gpud service.
///
/// Emits only the real markers observed from the gpud service:
///   gpud: virtio-gpu probed → gpud: ready
pub struct GpudContract {
    id: Option<ServiceId>,
}

impl GpudContract {
    pub fn probe_only() -> Self {
        Self { id: None }
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

        bus.emit_marker(id, "gpud: virtio-gpu probed");
        bus.emit_marker(id, "gpud: ready");

        Ok(())
    }
}
