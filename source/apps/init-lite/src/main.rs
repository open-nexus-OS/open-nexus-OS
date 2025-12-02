#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std,
    no_main
)]

// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod os_entry {
    use nexus_init::os_payload::{self, ReadyNotifier};
    use nexus_log;
    use nexus_service_entry::declare_entry;

    declare_entry!(init_main);

    mod services {
        include!(concat!(env!("OUT_DIR"), "/services.rs"));
    }

    type Result<T> = core::result::Result<T, os_payload::InitError>;

    fn init_main() -> Result<()> {
        let notifier = ReadyNotifier::new(|| ());
        match os_payload::service_main_loop_images(services::SERVICE_IMAGES, notifier) {
            Ok(()) => unreachable!(),
            Err(err) => {
                nexus_log::error("init", |line| {
                    line.text("init-lite: fatal bootstrap err=");
                    os_payload::describe_init_error(line, &err);
                });
                Err(err)
            }
        }
    }
}
