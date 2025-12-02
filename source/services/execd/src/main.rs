#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std,
    no_main
)]

//! CONTEXT: Execd daemon entrypoint wiring default transport to shared service logic

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> execd::LiteResult<()> {
    execd::service_main_loop(execd::ReadyNotifier::new(|| {}))
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() -> ! {
    execd::touch_schemas();
    if let Err(err) = execd::service_main_loop(execd::ReadyNotifier::new(|| ())) {
        eprintln!("execd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
