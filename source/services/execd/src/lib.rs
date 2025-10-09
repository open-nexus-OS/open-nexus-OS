//! Execd stub daemon shared entry helpers.

#![forbid(unsafe_code)]

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '--cfg nexus_env=\"os\"'.");

#[cfg(any(nexus_env = "host", nexus_env = "os"))]
fn run<R: FnOnce()>(notify: R) -> ! {
    println!("execd: ready");
    notify();
    loop {
        core::hint::spin_loop();
    }
}

/// Runs the execd stub until it is terminated by the runtime.
#[cfg(nexus_env = "host")]
pub fn daemon_main<R: FnOnce()>(notify: R) -> ! {
    run(notify)
}

/// Runs the execd stub until it is terminated by the runtime.
#[cfg(nexus_env = "os")]
pub fn daemon_main<R: FnOnce()>(notify: R) -> ! {
    run(notify)
}
