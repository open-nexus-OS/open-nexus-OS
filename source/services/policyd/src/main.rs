//! Bin wrapper for the policyd stub daemon entry point.

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '--cfg nexus_env=\"os\"'.");

#[cfg(nexus_env = "host")]
fn main() {
    let _ = policyd::daemon_main(|| {});
}

#[cfg(nexus_env = "os")]
fn main() -> ! {
    policyd::daemon_main(|| {})
}
