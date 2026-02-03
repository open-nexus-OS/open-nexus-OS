// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: updated host daemon (std) – placeholder loop for future Cap'n Proto RPCs
//! OWNERS: @services-team
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//!
//! ADR: docs/adr/0024-updates-ab-packaging-architecture.md

use std::io;

/// Result type surfaced by the host daemon.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors emitted by the host daemon.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Host path not wired yet.
    #[error("updated host daemon not wired yet")]
    Unsupported,
    /// I/O failure.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

/// Runs the host daemon (placeholder).
pub fn daemon_main<F: FnOnce() + Send>(ready: F) {
    ready();
    eprintln!("updated: host daemon placeholder (not wired)");
}

/// Touches schema types to keep host parity.
pub fn touch_schemas() {}
