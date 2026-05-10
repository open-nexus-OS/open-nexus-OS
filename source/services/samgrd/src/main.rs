#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

//! CONTEXT: SAMGR daemon entrypoint wiring default transport to shared server logic
//! INTENT: Wire host and os-lite entrypoints into samgrd service loops
//! IDL (target): register(name, endpoint), resolve(name), heartbeat(endpoint)
//! DEPS: samgrd std/os-lite backends, nexus-service-entry
//! READINESS: emit "samgrd: ready" after service loop starts
//! TESTS: tests/e2e/tests/samgrd_roundtrip.rs

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> samgrd::LiteResult<()> {
    samgrd::service_main_loop(samgrd::ReadyNotifier::new(|| {}))
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() -> ! {
    samgrd::touch_schemas();
    if let Err(err) = samgrd::service_main_loop(samgrd::ReadyNotifier::new(|| ())) {
        eprintln!("samgrd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
