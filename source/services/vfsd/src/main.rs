#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std,
    no_main
)]

//! CONTEXT: Vfsd entrypoint wiring default transport to shared service logic

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> vfsd::Result<()> {
    vfsd::service_main_loop(vfsd::ReadyNotifier::new(|| {}))
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {
    if let Err(err) = vfsd::service_main_loop(vfsd::ReadyNotifier::new(|| {})) {
        eprintln!("vfsd: exited with error: {err}");
        std::process::exit(1);
    }
}
