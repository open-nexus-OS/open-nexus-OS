//! Bin wrapper delegating to the execd stub library entry point.

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '--cfg nexus_env=\"os\"'.");

#[cfg(nexus_env = "host")]
fn main() {
    let _ = execd::daemon_main(|| {});
}

#[cfg(nexus_env = "os")]
fn main() -> ! {
    execd::daemon_main(|| {})
}
