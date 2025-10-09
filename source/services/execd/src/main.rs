//! Bin wrapper delegating to the execd stub library entry point.

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '--cfg nexus_env=\"os\"'.");

#[cfg(nexus_env = "host")]
fn main() {
    let notifier = execd::ReadyNotifier::new(|| {});
    let _ = execd::service_main_loop(notifier);
}

#[cfg(nexus_env = "os")]
fn main() {
    let notifier = execd::ReadyNotifier::new(|| {});
    let _ = execd::service_main_loop(notifier);
    loop { core::hint::spin_loop(); }
}
