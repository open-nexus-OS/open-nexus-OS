// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Service Contracts für Integration Chain Tests.
//! OWNERS: @tools-team

#![allow(clippy::unwrap_used)]

mod gpud;
mod inputd;
mod windowd;

use crate::chain::ServiceId;

#[allow(unused_imports)]
pub use gpud::GpudContract;
#[allow(unused_imports)]
pub use inputd::InputdContract;
#[allow(unused_imports)]
pub use windowd::WindowdContract;

/// Fehler, der von einem Contract während der Chain-Ausführung zurückgegeben wird.
#[derive(Debug, Clone)]
pub struct ContractError {
    pub service: ServiceId,
    pub message: String,
}

impl ContractError {
    pub fn new(service: ServiceId, message: impl Into<String>) -> Self {
        Self { service, message: message.into() }
    }
}

/// Handle für eine simulierte Capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SimCapHandle(#[allow(dead_code)] pub u32);

/// Beschreibung einer Capability, die ein Service beim Start erhält.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SimCapDesc {
    /// Eine VMO mit angegebener Größe.
    Vmo { name: String, size: usize },
    /// Ein IPC-Endpoint zu einem anderen Service.
    Endpoint { target: String },
    /// Eine Reply-Cap für asynchrone Antworten.
    Reply { name: String },
}

/// Ein Service, der in einer simulierten Chain-Umgebung laufen kann.
/// Erfordert `Send` für parallele Ausführung via tokio::spawn.
pub trait Contract: Send {
    /// Eindeutiger Name (z.B. "fbdevd", "windowd").
    fn service_name(&self) -> &'static str;

    /// Initiale Capabilities, die der Service beim Start erhält.
    fn initial_caps(&self) -> Vec<SimCapDesc> {
        Vec::new()
    }

    /// Setzt die Service-ID (wird vom ChainRunner aufgerufen).
    fn set_service_id(&mut self, id: ServiceId);

    /// Führt den Service aus. Läuft bis zur Termination oder bis
    /// der ChainRunner die Chain beendet.
    fn run(&mut self, bus: &mut crate::chain::SimIpcBus) -> Result<(), ContractError>;
}
