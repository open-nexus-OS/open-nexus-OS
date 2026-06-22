// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SystemUI service entrypoint. On the OS target it boots as a service
//! (`nexus_service_entry::declare_entry!`) and resolves the default shell from the
//! manifest registry; on host it is a CLI seed (`systemui::run`).
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! DEPENDS_ON: systemui::service_boot (os-lite), nexus-service-entry (os-lite)

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> core::result::Result<(), systemui::SystemUiError> {
    systemui::service_boot()
}

#[cfg(nexus_env = "host")]
fn main() {
    systemui::run();
}
