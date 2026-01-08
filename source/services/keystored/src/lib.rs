#![forbid(unsafe_code)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

#[cfg(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

#[cfg(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod os_stub;

#[cfg(all(
    feature = "os-lite",
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none"
))]
pub use os_stub::*;

// full_impl requires idl-capnp and only builds on host targets
#[cfg(all(
    feature = "idl-capnp",
    not(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))
))]
mod full_impl;

#[cfg(all(
    feature = "idl-capnp",
    not(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))
))]
pub use full_impl::*;
