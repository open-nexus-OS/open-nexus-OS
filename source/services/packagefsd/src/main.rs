#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std,
    no_main
)]

//! CONTEXT: Packagefsd entrypoint wiring default transport to shared service logic
//! Package file system daemon entrypoint.

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> packagefsd::LiteResult<()> {
    packagefsd::service_main_loop(packagefsd::ReadyNotifier::new(|| ()))
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() -> ! {
    #[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
    packagefsd::touch_schemas();
    if let Err(err) = packagefsd::service_main_loop(packagefsd::ReadyNotifier::new(|| ())) {
        eprintln!("packagefsd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
