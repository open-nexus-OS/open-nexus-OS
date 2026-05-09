// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Stable marker strings for the service-owned display path.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Verified via host tests and visible-bootstrap QEMU proofs.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use alloc::{format, string::String};

pub const READY_MARKER: &str = "fbdevd: ready";
pub const MAP_OK_MARKER: &str = "fbdevd: map ok";
pub const RAMFB_CONFIGURED_MARKER: &str = "fbdevd: ramfb configured";
pub const FLUSH_OK_MARKER: &str = "fbdevd: flush ok";

pub fn vsync_marker(seq: u64) -> String {
    format!("fbdevd: vsync seq={seq}")
}
