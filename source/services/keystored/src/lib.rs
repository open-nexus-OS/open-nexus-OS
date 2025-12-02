#![forbid(unsafe_code)]
#![cfg_attr(
    all(
        feature = "os-lite",
        nexus_env = "os",
        target_arch = "riscv64",
        target_os = "none"
    ),
    no_std
)]

#[cfg(all(
    feature = "os-lite",
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none"
))]
extern crate alloc;

#[cfg(all(
    feature = "os-lite",
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none"
))]
mod os_stub;

#[cfg(all(
    feature = "os-lite",
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none"
))]
pub use os_stub::*;

#[cfg(not(all(
    feature = "os-lite",
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none"
)))]
mod full_impl;

#[cfg(not(all(
    feature = "os-lite",
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none"
)))]
pub use full_impl::*;
