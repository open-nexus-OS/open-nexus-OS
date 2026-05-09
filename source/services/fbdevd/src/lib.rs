// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `fbdevd` service-owned QEMU `ramfb` scanout path for visible display proofs.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p fbdevd -- --nocapture`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![cfg_attr(not(all(nexus_env = "os", target_os = "none")), forbid(unsafe_code))]

extern crate alloc;

mod backend;
mod error;
mod markers;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
mod os_lite;
mod protocol;
mod reactor;
mod scanout;
mod service;
mod vsync;

pub use backend::framebuffer::{validate_framebuffer_cap, validate_handoff};
pub use backend::ramfb::{
    dma_transfer_complete, encode_ramfb_config, encode_ramfb_dma_request, require_fw_cfg_signature,
    validate_dma_capability, validate_ramfb_file, DmaCapabilityInfo, RamfbFileInfo,
};
pub use error::FbdevdError;
pub use markers::{
    vsync_marker, FLUSH_OK_MARKER, MAP_OK_MARKER, RAMFB_CONFIGURED_MARKER, READY_MARKER,
};
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub use os_lite::service_main_loop;
pub use reactor::{live_dirty_rows, DirtyRows, DisplayReactor, ReactorProgress, TickBudget};
pub use scanout::DisplayScanout;
pub use service::FbdevService;
pub use vsync::VsyncCadence;

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
pub fn run() {
    println!("fbdevd: ready");
}
