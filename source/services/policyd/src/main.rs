#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std,
    no_main
)]

//! CONTEXT: Policy daemon entrypoint wiring to service logic
//! INTENT: Policy/entitlement/DAC checks, audit
//! IDL (target): checkPermission(subject,cap), addPolicy(entry), audit(record)
//! DEPS: keystored/identityd (crypto/IDs)
//! READINESS: print "policyd: ready"; register/heartbeat with samgr
//! TESTS: checkPermission loopback; deny/allow paths
//! Bin wrapper wiring policyd's daemon entry point.

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!(
    "nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '--cfg nexus_env=\"os\"'.",
);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> policyd::LiteResult<()> {
    policyd::service_main_loop(policyd::ReadyNotifier::new(|| {}))
}

#[cfg(nexus_env = "host")]
fn main() {
    policyd::daemon_main(|| {});
}

#[cfg(all(
    nexus_env = "os",
    not(all(target_arch = "riscv64", target_os = "none"))
))]
fn main() {
    policyd::touch_schemas();
    let notifier = policyd::ReadyNotifier::new(|| {});
    let _ = policyd::service_main_loop(notifier);
    loop {
        core::hint::spin_loop();
    }
}
