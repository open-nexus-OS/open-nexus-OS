//! Bin wrapper wiring policyd's daemon entry point.

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!(
    "nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '--cfg nexus_env=\"os\"'.",
);

#[cfg(nexus_env = "host")]
fn main() {
    policyd::daemon_main(|| {});
}

#[cfg(nexus_env = "os")]
fn main() {
    policyd::touch_schemas();
    let notifier = policyd::ReadyNotifier::new(|| {});
    let _ = policyd::service_main_loop(notifier);
    loop {
        core::hint::spin_loop();
    }
}
