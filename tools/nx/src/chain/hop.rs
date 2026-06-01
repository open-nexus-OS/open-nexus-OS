// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

/// Ein Hop in der Integration-Chain.
/// Wird erfüllt, wenn `marker` innerhalb von `timeout` im simulierten System erscheint.
#[derive(Debug, Clone)]
pub struct Hop {
    /// Eindeutiger Name (z.B. "fbdevd-ready").
    pub name: String,
    /// Der erwartete Marker-String (z.B. "fbdevd: ready").
    pub marker: String,
    /// Maximale Wartezeit für diesen Marker.
    pub timeout: Duration,
    /// Indizes der Hops, die VOR diesem Hop erfüllt sein müssen.
    /// Leerer Vec = keine Abhängigkeiten.
    pub depends_on: Vec<usize>,
    /// Wenn true: Fehler ist non-fatal, Chain läuft weiter.
    pub optional: bool,
    /// Menschliche Beschreibung, was dieser Hop prüft.
    pub description: String,
    /// Ergebnis nach der Chain-Ausführung.
    pub result: Option<HopResult>,
}

impl Hop {
    /// Setzt die Abhängigkeit auf einen vorherigen Hop (per Index).
    pub fn after(&mut self, dep_index: usize) -> &mut Self {
        self.depends_on.push(dep_index);
        self
    }

    /// Setzt die Beschreibung.
    pub fn describe(&mut self, desc: &str) -> &mut Self {
        self.description = desc.to_string();
        self
    }

    /// Markiert den Hop als optional.
    pub fn optional(&mut self) -> &mut Self {
        self.optional = true;
        self
    }
}

/// Ergebnis eines einzelnen Hops.
#[derive(Debug, Clone)]
pub struct HopResult {
    pub hop_name: String,
    pub marker: String,
    /// Wurde der Marker innerhalb des Timeouts gesehen?
    pub seen: bool,
    /// Verstrichene Mikrosekunden seit Chain-Start.
    pub elapsed_us: u64,
    /// Waren alle Abhängigkeiten erfüllt?
    pub order_ok: bool,
}

/// Warum ein Hop fehlgeschlagen ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HopFailure {
    /// Der Marker wurde nicht innerhalb des Timeouts gesehen.
    MarkerNotFound { marker: String, last_marker_seen: Option<String> },
    /// Die Reihenfolge war falsch (Marker erschien vor seinem Vorgänger).
    OrderViolation { marker: String, expected_after: String, appeared_before: String },
    /// Der Service ist mit einem Fehler abgestürzt.
    ServicePanic { service: String, error: String },
}

// ── Hilfsfunktion ──

/// Erzeugt eine Duration aus Millisekunden.
pub const fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}
