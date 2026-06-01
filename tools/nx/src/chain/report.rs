// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::chain::hop::{HopFailure, HopResult};
use crate::chain::ServiceId;
use std::fmt;

/// Ergebnis einer Chain-Ausführung.
#[derive(Debug, Clone)]
pub struct ChainReport {
    pub chain_name: String,
    pub status: ChainStatus,
    pub hops: Vec<HopResult>,
    pub service_errors: Vec<ServiceError>,
    pub timeline: Vec<TimelineEntry>,
}

/// Gesamtstatus der Chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainStatus {
    Passed,
    Failed { failed_hop: usize, reason: HopFailure },
    Timeout { last_hop: usize },
}

/// Ein Service-Fehler während der Chain-Ausführung.
#[derive(Debug, Clone)]
pub struct ServiceError {
    pub service: ServiceId,
    pub message: String,
}

/// Ein Eintrag in der Event-Timeline.
#[derive(Debug, Clone)]
pub struct TimelineEntry {
    pub timestamp_us: u64,
    pub service: ServiceId,
    pub event: TimelineEvent,
}

/// Ein Event in der Timeline.
#[derive(Debug, Clone)]
pub enum TimelineEvent {
    MarkerEmitted(String),
    IpcSend { to: ServiceId, op: u8 },
    IpcRecv { from: ServiceId, op: u8 },
    CapTransfer { from: ServiceId, to: ServiceId, slot: u32 },
    Error(String),
}

impl ChainReport {
    /// Produziert eine menschenlesbare Fehlerdiagnostik.
    /// Im Erfolgsfall: kurze Bestätigung. Im Fehlerfall: detaillierte Timeline.
    pub fn diagnostic(&self) -> String {
        let mut out = String::new();
        out.push_str("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
        out.push_str(&format!("Chain: {}\n", self.chain_name));

        match &self.status {
            ChainStatus::Passed => {
                out.push_str(&format!(
                    "Status: PASSED ✅  ({}/{} hops ok)\n",
                    self.hops.iter().filter(|h| h.seen).count(),
                    self.hops.len()
                ));
            }
            ChainStatus::Failed { failed_hop, reason } => {
                out.push_str(&format!(
                    "Status: FAILED ❌  at hop {}/{}\n",
                    failed_hop + 1,
                    self.hops.len()
                ));
                out.push_str(&format!("Reason: {}\n", reason));
            }
            ChainStatus::Timeout { last_hop } => {
                out.push_str(&format!(
                    "Status: TIMEOUT ⏱  after hop {}/{}\n",
                    last_hop + 1,
                    self.hops.len()
                ));
            }
        }
        out.push('\n');

        // Hop-Ergebnisse
        out.push_str("Hop Results:\n");
        for (i, hop) in self.hops.iter().enumerate() {
            let icon = if hop.seen {
                "✅"
            } else if self.is_hop_skipped(i) {
                "⏭"
            } else {
                "❌"
            };
            out.push_str(&format!(
                "  {}. {:30} {}  T+{:.1}s\n",
                i + 1,
                hop.hop_name,
                icon,
                hop.elapsed_us as f64 / 1_000_000.0
            ));
        }
        out.push('\n');

        // Fehlerdetails
        if let ChainStatus::Failed { reason, .. } = &self.status {
            out.push_str(&format!("Failure Detail:\n  {}\n\n", reason));
        }

        // Timeline (letzte 15 Events)
        out.push_str("Timeline (recent):\n");
        let start = self.timeline.len().saturating_sub(15);
        for entry in &self.timeline[start..] {
            out.push_str(&format!(
                "  T+{:.4}s  {:10}  {}\n",
                entry.timestamp_us as f64 / 1_000_000.0,
                format!("svc-{}", entry.service.0),
                entry.event
            ));
        }
        out.push('\n');

        // Service-Fehler
        if !self.service_errors.is_empty() {
            out.push_str("Service Errors:\n");
            for err in &self.service_errors {
                out.push_str(&format!("  svc-{}: {}\n", err.service.0, err.message));
            }
            out.push('\n');
        }

        out.push_str("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
        out
    }

    fn is_hop_skipped(&self, idx: usize) -> bool {
        if let ChainStatus::Failed { failed_hop, .. } = &self.status {
            idx > *failed_hop
        } else {
            false
        }
    }
}

impl fmt::Display for HopFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HopFailure::MarkerNotFound { marker, last_marker_seen } => {
                write!(f, "Marker \"{marker}\" not seen")?;
                if let Some(last) = last_marker_seen {
                    write!(f, " — last marker: \"{last}\"")?;
                }
                Ok(())
            }
            HopFailure::OrderViolation { marker, expected_after, appeared_before } => {
                write!(
                    f,
                    "Order violation: \"{marker}\" expected after \"{expected_after}\" but appeared before \"{appeared_before}\""
                )
            }
            HopFailure::ServicePanic { service, error } => {
                write!(f, "Service \"{service}\" panicked: {error}")
            }
        }
    }
}

impl fmt::Display for TimelineEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimelineEvent::MarkerEmitted(m) => write!(f, "marker \"{m}\""),
            TimelineEvent::IpcSend { to, op } => write!(f, "IPC send → svc-{} (op={})", to.0, op),
            TimelineEvent::IpcRecv { from, op } => {
                write!(f, "IPC recv ← svc-{} (op={})", from.0, op)
            }
            TimelineEvent::CapTransfer { from, to, slot } => {
                write!(f, "cap transfer svc-{} → svc-{} (slot={})", from.0, to.0, slot)
            }
            TimelineEvent::Error(msg) => write!(f, "ERROR: {msg}"),
        }
    }
}
