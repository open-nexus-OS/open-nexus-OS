#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

//! CONTEXT: Keystored daemon entrypoint wiring default transport to service logic.

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> keystored::LiteResult<()> {
    keystored::service_main_loop(keystored::ReadyNotifier::new(|| {}))
}

#[cfg(nexus_env = "host")]
fn main() {
    keystored::daemon_main(|| {});
}
