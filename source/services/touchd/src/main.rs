// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `touchd` daemon entrypoint forwarding to the service runtime.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p touchd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]
#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> Result<(), nexus_abi::AbiError> {
    // RFC-0068: fold routine debug_println markers into one `touchd N/N` verdict (interactive boots;
    // proof stays raw).
    nexus_abi::service_verdict_arm();
    if let Ok(bounds) = touch::TouchBounds::new(64, 48) {
        let mut service =
            touchd::TouchdService::new(bounds, touchd::SyntheticTouchMode::ProofFixture);
        service.register_device(touchd::TouchDeviceId::new(1));
        if service.ready() {
            nexus_abi::debug_println("touchd: os service payload ready")?;
        }
    }
    nexus_abi::service_verdict_flush("touchd");
    Ok(())
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {
    touchd::run();
}
