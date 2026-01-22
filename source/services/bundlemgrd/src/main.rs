#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

//! CONTEXT: Bundle manager daemon entrypoint wiring to service logic
//! INTENT: Wire host and os-lite entrypoints into bundlemgrd service loops
//! IDL (target): installBundle, queryBundle, getPayload
//! DEPS: bundlemgrd backends, nexus-service-entry
//! READINESS: emit "bundlemgrd: ready" after service loop starts
//! TESTS: tests/e2e/tests/bundlemgrd_roundtrip.rs

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
