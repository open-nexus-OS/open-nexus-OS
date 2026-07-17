// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Liveness observer — polls samgrd for service health status.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder
//!
//! Reads service registration status from samgrd without initiating
//! control-plane IPC (uses logd markers as the health signal).

// RFC-0061 M4 pure-observer toolkit (liveness-checker): declared observer API surface,
// kept per ADR-0027 until the observer ladder wires it in — module-scoped
// allow, not crate-level (repo rule).
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

/// Service health status as observed from logd markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServiceHealth {
    /// Service emitted its readiness marker.
    Ready,
    /// Service has not yet emitted a readiness marker.
    NotReady,
    /// Service emitted a failure marker.
    Failed,
}

/// Check whether a service is healthy by looking for its readiness marker
/// in the marker stream.
#[derive(Debug, Default)]
pub(crate) struct LivenessChecker {
    /// Set of services known to be ready.
    ready_services: Vec<(&'static str, bool)>,
}

impl LivenessChecker {
    /// Create a liveness checker for the given service list.
    pub fn new(services: &[&'static str]) -> Self {
        Self { ready_services: services.iter().map(|&s| (s, false)).collect() }
    }

    /// Mark a service as ready when its marker is observed.
    pub fn mark_ready(&mut self, service: &str) {
        for (name, ready) in &mut self.ready_services {
            if *name == service {
                *ready = true;
                return;
            }
        }
    }

    /// Check if all registered services are ready.
    pub fn all_ready(&self) -> bool {
        self.ready_services.iter().all(|(_, ready)| *ready)
    }

    /// Check if a specific service is ready.
    pub fn is_ready(&self, service: &str) -> bool {
        self.ready_services.iter().any(|(name, ready)| *name == service && *ready)
    }

    /// Return list of services not yet ready.
    pub fn pending(&self) -> Vec<&'static str> {
        self.ready_services.iter().filter(|(_, ready)| !*ready).map(|(name, _)| *name).collect()
    }
}

/// Parse a service-ready marker from a UART line.
///
/// Recognises markers of the form `servicename: ready`.
pub(crate) fn parse_ready_marker(line: &[u8]) -> Option<&str> {
    let line = core::str::from_utf8(line).ok()?;
    let line = line.trim_end();
    if let Some(name) = line.strip_suffix(": ready") {
        Some(name.trim())
    } else {
        None
    }
}

/// Parse a service-failure marker from a UART line.
pub(crate) fn parse_failure_marker(line: &[u8]) -> Option<&str> {
    let line = core::str::from_utf8(line).ok()?;
    let line = line.trim_end();
    if let Some(rest) = line.strip_prefix("!fatal ") {
        Some(rest.trim())
    } else if let Some(rest) = line.strip_prefix("!fatal-err ") {
        Some(rest.trim())
    } else {
        None
    }
}
