#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"),
    no_std
)]

//! CONTEXT: OS backend implementation for the `nexus-net` sockets facade (smoltcp over virtio-net).
//! OWNERS: @runtime
//! STATUS: Bring-up
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered indirectly by `selftest-client` QEMU marker suite.
//!
//! Design constraints:
//! - The `nexus-net` crate is the **contract**; this crate is an OS-only backend.
//! - Unsafe is permitted here for MMIO/DMA and is kept narrowly scoped.
//!
//! Host builds:
//! - This crate must still **compile on the host** so `cargo check --workspace --all-targets`
//!   and host-first CI tooling can validate the full workspace.
//! - The actual OS backend implementation is only available under the os-lite + riscv64 + none
//!   configuration below. Host code should depend on the `nexus-net` contract crate instead.

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
mod smoltcp_virtio;

#[cfg(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
))]
pub use smoltcp_virtio::SmoltcpVirtioNetStack;

// OS-only concrete socket types (useful for owner services like netstackd).
#[cfg(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
))]
pub use smoltcp_virtio::{DhcpConfig, OsTcpListener, OsTcpStream, OsUdpSocket};

// Non-OS builds intentionally expose no API.
// Consumers must use the `nexus-net` contract crate on the host.

// If someone explicitly enables the OS backend feature on the wrong platform/config, fail with a
// clear error message (without breaking host-first workspace diagnostics).
#[cfg(all(
    feature = "os-lite",
    not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))
))]
compile_error!(
    "nexus-net-os (feature=os-lite) is only supported for the OS riscv64 bare-metal build. \
Build with nexus_env=\"os\", target=riscv64*-unknown-none-elf, and feature os-lite."
);
