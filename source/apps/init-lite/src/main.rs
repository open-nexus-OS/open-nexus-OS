#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod os_entry {
    use nexus_init::os_payload::{self, ReadyNotifier};
    use nexus_service_entry::declare_entry;

    declare_entry!(init_main);

    type Result<T> = core::result::Result<T, os_payload::InitError>;

    fn init_main() -> Result<()> {
        let notifier = ReadyNotifier::new(|| ());
        // ADR-0081: the core services are files of the bootfs the kernel handed over — no
        // program is embedded here.
        let images = match os_payload::bootfs_images() {
            Ok(images) => images,
            Err(err) => os_payload::fatal_err(err),
        };
        match os_payload::service_main_loop_images(images, notifier) {
            Ok(()) => unreachable!(),
            Err(err) => os_payload::fatal_err(err),
        }
    }
}
