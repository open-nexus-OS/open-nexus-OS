#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

//! CONTEXT: Bundle Manager daemon entrypoint wiring to service logic
//! INTENT: Application bundle install/query/serve, manifest parsing, capability checks
//! IDL (target): installBundle(path), queryBundle(id), getCapabilities(id), uninstallBundle(id)
//! DEPS: keystored (signature verification), policyd (capability checks), vfsd (file access)
//! READINESS: print "bundlemgrd: ready"; register/heartbeat with samgr
//! TESTS: installBundle mock; queryBundle returns manifest
//! Bundle manager daemon entrypoint wiring default transport to the shared service logic.

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> bundlemgrd::LiteResult<()> {
    bundlemgrd::service_main_loop(
        bundlemgrd::ReadyNotifier::new(|| {}),
        bundlemgrd::ArtifactStore::new(),
    )
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() -> ! {
    bundlemgrd::touch_schemas();
    let artifacts = bundlemgrd::ArtifactStore::new();
    if let Err(err) =
        bundlemgrd::service_main_loop(bundlemgrd::ReadyNotifier::new(|| ()), artifacts)
    {
        eprintln!("bundlemgrd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
