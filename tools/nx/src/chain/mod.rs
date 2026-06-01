// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Integration Chain Test Framework — simuliert Service-IPC im Host-Prozess.
//! OWNERS: @tools-team
//! STATUS: Experimental
//! API_STABILITY: Unstable
//!
//! Ersetzt Source-Scraping-Tests durch Contract-basierte Simulation.
//! Services laufen als Futures im selben Prozess, kommunizieren über
//! einen In-Memory-IPC-Bus. Hop-Marker validieren die Kette.
//!
//! ADR: docs/adr/0030-integration-chain-test-framework.md

#![allow(clippy::unwrap_used, clippy::expect_used)]

pub mod contract;
pub mod hop;
pub mod report;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use contract::{Contract, ContractError, SimCapDesc, SimCapHandle};
pub use hop::{Hop, HopFailure, HopResult};
pub use report::{ChainReport, ChainStatus, ServiceError, TimelineEntry, TimelineEvent};

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ═══════════════════════════════════════════════════════════════════
// SimIpcBus — In-Memory-IPC-Bus für Chain-Tests
// ═══════════════════════════════════════════════════════════════════

/// Service-Identifier im simulierten System.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceId(pub u32);

/// Eine IPC-Nachricht im simulierten Bus.
#[derive(Debug, Clone)]
pub struct IpcMessage {
    pub from: ServiceId,
    pub to: ServiceId,
    pub op: u8,
    pub payload: Vec<u8>,
    pub cap: Option<SimCapHandle>,
}

/// Simulierter IPC-Bus. Services senden/empfangen Frames + Caps.
#[derive(Debug, Default)]
pub struct SimIpcBus {
    queues: HashMap<ServiceId, VecDeque<IpcMessage>>,
    name_to_id: HashMap<String, ServiceId>,
    id_to_name: HashMap<ServiceId, String>,
    next_id: u32,
    cap_table: SimCapTable,
    /// Alle emittierten Marker (für Hop-Validierung).
    markers: Vec<MarkerRecord>,
    /// Timeline aller Events (für Fehlerdiagnostik).
    timeline: Vec<TimelineEntry>,
}

#[derive(Debug, Clone)]
pub struct MarkerRecord {
    pub marker: String,
    pub service: ServiceId,
    pub timestamp_us: u64,
}

#[derive(Debug, Default)]
pub(crate) struct SimCapTable {
    entries: HashMap<u32, SimCapEntry>,
    next_handle: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct SimCapEntry {
    #[allow(dead_code)]
    pub handle: SimCapHandle,
    pub kind: SimCapKind,
    pub owner: ServiceId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimCapKind {
    Vmo { size: usize },
    Endpoint,
    Reply,
}

impl SimIpcBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registriert einen Service-Namen und gibt die ID zurück.
    pub fn register_service(&mut self, name: &str) -> ServiceId {
        if let Some(id) = self.name_to_id.get(name) {
            return *id;
        }
        let id = ServiceId(self.next_id);
        self.next_id += 1;
        self.name_to_id.insert(name.to_string(), id);
        self.id_to_name.insert(id, name.to_string());
        self.queues.entry(id).or_default();
        id
    }

    /// Sendet eine Nachricht an einen Service.
    pub fn send(
        &mut self,
        from: ServiceId,
        to: ServiceId,
        op: u8,
        payload: Vec<u8>,
        cap: Option<SimCapHandle>,
    ) {
        let now = now_us();
        self.timeline.push(TimelineEntry {
            timestamp_us: now,
            service: from,
            event: TimelineEvent::IpcSend { to, op },
        });
        self.queues.entry(to).or_default().push_back(IpcMessage { from, to, op, payload, cap });
    }

    /// Empfängt eine Nachricht (non-blocking). Gibt `None` zurück, wenn die Queue leer ist.
    pub fn recv(&mut self, service: ServiceId) -> Option<IpcMessage> {
        let msg = self.queues.entry(service).or_default().pop_front()?;
        let now = now_us();
        self.timeline.push(TimelineEntry {
            timestamp_us: now,
            service,
            event: TimelineEvent::IpcRecv { from: msg.from, op: msg.op },
        });
        Some(msg)
    }

    /// Blockiert, bis eine Nachricht verfügbar ist oder das Timeout abläuft.
    /// In der simulierten Umgebung: pollt mit kurzen Pausen.
    pub async fn recv_timeout(
        &mut self,
        service: ServiceId,
        timeout: Duration,
    ) -> Option<IpcMessage> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(msg) = self.recv(service) {
                return Some(msg);
            }
            if Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_micros(100)).await;
        }
    }

    /// Emittiert einen Marker im Namen eines Services.
    pub fn emit_marker(&mut self, service: ServiceId, marker: &str) {
        let now = now_us();
        self.markers.push(MarkerRecord { marker: marker.to_string(), service, timestamp_us: now });
        self.timeline.push(TimelineEntry {
            timestamp_us: now,
            service,
            event: TimelineEvent::MarkerEmitted(marker.to_string()),
        });
    }

    /// Gibt alle Marker zurück, deren Text den gegebenen String enthält.
    pub fn find_markers(&self, pattern: &str) -> Vec<&MarkerRecord> {
        self.markers.iter().filter(|m| m.marker.contains(pattern)).collect()
    }

    /// Prüft, ob ein bestimmter Marker emittiert wurde.
    pub fn has_marker(&self, marker: &str) -> bool {
        self.markers.iter().any(|m| m.marker == marker)
    }

    /// Gibt die Service-ID für einen Namen zurück.
    pub fn service_id(&self, name: &str) -> Option<ServiceId> {
        self.name_to_id.get(name).copied()
    }

    /// Gibt den Namen für eine Service-ID zurück.
    pub fn service_name(&self, id: ServiceId) -> &str {
        self.id_to_name.get(&id).map(|s| s.as_str()).unwrap_or("unknown")
    }

    // ── Capability-Table ──

    /// Alloziert eine neue VMO-Capability.
    pub fn alloc_vmo(&mut self, owner: ServiceId, size: usize) -> SimCapHandle {
        let handle = SimCapHandle(self.cap_table.next_handle);
        self.cap_table.next_handle += 1;
        self.cap_table
            .entries
            .insert(handle.0, SimCapEntry { handle, kind: SimCapKind::Vmo { size }, owner });
        handle
    }

    /// Klont eine Capability.
    pub fn cap_clone(&mut self, handle: SimCapHandle) -> Option<SimCapHandle> {
        let entry = self.cap_table.entries.get(&handle.0)?;
        let new_handle = SimCapHandle(self.cap_table.next_handle);
        self.cap_table.next_handle += 1;
        self.cap_table.entries.insert(
            new_handle.0,
            SimCapEntry { handle: new_handle, kind: entry.kind, owner: entry.owner },
        );
        Some(new_handle)
    }

    /// Transferiert eine Capability zu einem neuen Owner.
    pub fn cap_transfer(&mut self, handle: SimCapHandle, new_owner: ServiceId) -> bool {
        if let Some(entry) = self.cap_table.entries.get_mut(&handle.0) {
            let old = entry.owner;
            entry.owner = new_owner;
            let now = now_us();
            self.timeline.push(TimelineEntry {
                timestamp_us: now,
                service: new_owner,
                event: TimelineEvent::CapTransfer { from: old, to: new_owner, slot: handle.0 },
            });
            true
        } else {
            false
        }
    }

    /// Gibt die Timeline für die Fehlerdiagnostik zurück.
    pub fn timeline(&self) -> &[TimelineEntry] {
        &self.timeline
    }

    /// Gibt alle Marker zurück.
    pub fn markers(&self) -> &[MarkerRecord] {
        &self.markers
    }
}

/// Simulierte Uhr. Liefert Mikrosekunden seit Chain-Start.
#[derive(Debug, Default)]
pub struct SimClock {
    #[allow(dead_code)]
    start: Option<Instant>,
}

impl SimClock {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self { start: Some(Instant::now()) }
    }

    pub fn now_us(&self) -> u64 {
        self.start.map(|s| s.elapsed().as_micros() as u64).unwrap_or(0)
    }
}

/// Globale Zeit (für nicht-Clock-abhängige Marker).
fn now_us() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_micros() as u64
}

// ═══════════════════════════════════════════════════════════════════
// ChainRunner
// ═══════════════════════════════════════════════════════════════════

/// Ein Service, der in einer simulierten Chain-Umgebung laufen kann.
pub struct ChainRunner {
    name: String,
    bus: Arc<Mutex<SimIpcBus>>,
    #[allow(dead_code)]
    clock: SimClock,
    contracts: Vec<Box<dyn Contract + Send>>,
    hops: Vec<Hop>,
}

impl ChainRunner {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            bus: Arc::new(Mutex::new(SimIpcBus::new())),
            clock: SimClock::new(),
            contracts: Vec::new(),
            hops: Vec::new(),
        }
    }

    /// Registriert einen Service-Contract (Name wird im Bus registriert).
    pub fn register(&mut self, mut contract: Box<dyn Contract + Send>) -> ServiceId {
        let name = contract.service_name().to_string();
        let mut bus = self.bus.lock().unwrap();
        let id = bus.register_service(&name);
        drop(bus);
        contract.set_service_id(id);
        self.contracts.push(contract);
        id
    }

    /// Fügt einen Hop-Marker hinzu.
    pub fn expect_marker(&mut self, marker: &str, timeout: Duration) -> &mut Hop {
        let hop = Hop {
            name: format!("hop-{}", self.hops.len()),
            marker: marker.to_string(),
            timeout,
            depends_on: Vec::new(),
            optional: false,
            description: String::new(),
            result: None,
        };
        self.hops.push(hop);
        self.hops.last_mut().unwrap()
    }

    /// Führt die Chain aus. Services laufen parallel via tokio::spawn,
    /// Hops werden sequential geprüft. Kein MutexGuard wird über einen await-Punkt gehalten.
    #[must_use = "ChainReport enthält den Test-Status — prüfe report.status"]
    pub async fn run(self) -> ChainReport {
        let bus = self.bus.clone();

        // 1. Alle Services parallel starten
        let mut handles = Vec::new();
        for mut contract in self.contracts {
            let bus_clone = bus.clone();
            handles.push(tokio::spawn(async move {
                let mut b = bus_clone.lock().unwrap();
                contract.run(&mut b)
            }));
        }

        // Give spawned services time to start and emit their markers.
        // Contracts are synchronous (no internal await), so they run to
        // completion once the tokio runtime schedules them. A short sleep
        // ensures markers are emitted before hop polling begins.
        tokio::time::sleep(Duration::from_millis(20)).await;

        // 2. Hops der Reihe nach prüfen (während Services parallel laufen)
        let mut results = Vec::new();
        for (i, hop) in self.hops.iter().enumerate() {
            let deadline = Instant::now() + hop.timeout;
            let mut seen = false;

            // Abhängigkeiten prüfen
            let deps_ok = hop
                .depends_on
                .iter()
                .all(|&dep_idx| results.get(dep_idx).map(|r: &HopResult| r.seen).unwrap_or(false));

            if !deps_ok {
                results.push(HopResult {
                    hop_name: hop.name.clone(),
                    marker: hop.marker.clone(),
                    seen: false,
                    elapsed_us: 0,
                    order_ok: false,
                });
                continue;
            }

            // Poll-Loop: MutexGuard wird VOR jedem await explizit gedroppt.
            loop {
                if Instant::now() >= deadline {
                    break;
                }
                let has_now = {
                    let b = bus.lock().unwrap();
                    b.has_marker(&hop.marker)
                }; // MutexGuard dropped here — before await
                if has_now {
                    seen = true;
                    break;
                }
                tokio::time::sleep(Duration::from_micros(500)).await;
            }

            let elapsed = if seen {
                let b = bus.lock().unwrap();
                b.markers()
                    .iter()
                    .find(|m| m.marker == hop.marker)
                    .map(|m| m.timestamp_us)
                    .unwrap_or(0)
            } else {
                0
            };

            results.push(HopResult {
                hop_name: hop.name.clone(),
                marker: hop.marker.clone(),
                seen,
                elapsed_us: elapsed,
                order_ok: deps_ok,
            });

            if !seen && !hop.optional {
                let timeline = {
                    let b = bus.lock().unwrap();
                    b.timeline().to_vec()
                };
                let last_marker = {
                    let b = bus.lock().unwrap();
                    b.markers().last().map(|m| m.marker.clone())
                };

                return ChainReport {
                    chain_name: self.name.clone(),
                    status: ChainStatus::Failed {
                        failed_hop: i,
                        reason: HopFailure::MarkerNotFound {
                            marker: hop.marker.clone(),
                            last_marker_seen: last_marker,
                        },
                    },
                    hops: results,
                    service_errors: Vec::new(),
                    timeline,
                };
            }
        }

        // Warte auf alle Services
        for handle in handles {
            let _ = handle.await;
        }

        let timeline = {
            let b = bus.lock().unwrap();
            b.timeline().to_vec()
        };

        ChainReport {
            chain_name: self.name.clone(),
            status: ChainStatus::Passed,
            hops: results,
            service_errors: Vec::new(),
            timeline,
        }
    }
}
