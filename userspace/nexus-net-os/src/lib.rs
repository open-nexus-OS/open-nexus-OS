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

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
mod smoltcp_virtio;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
pub use smoltcp_virtio::SmoltcpVirtioNetStack;

// OS-only concrete socket types (useful for owner services like netstackd).
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
pub use smoltcp_virtio::{OsTcpListener, OsTcpStream, OsUdpSocket};

// Host builds are intentionally unsupported: this is OS-only.
#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite")))]
compile_error!("nexus-net-os is OS-only. Build with nexus_env=\"os\" and feature os-lite.");

