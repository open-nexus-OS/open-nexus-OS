// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel syscall surface of nexus-abi — submodules per concern plus
//! the riscv64 ecall island (the only unsafe code in the crate). Items are
//! re-exported at the crate root, so `nexus_abi::yield_`, `nexus_abi::sched`,
//! etc. keep resolving unchanged (ADR-0051 hygiene pass).

pub mod caps;
pub mod debug;
pub mod ipc;
pub mod memory;
pub mod task;
pub mod time;
pub mod types;

#[cfg(nexus_env = "os")]
pub use caps::*;
pub use debug::*;
pub use ipc::*;
#[cfg(nexus_env = "os")]
pub use memory::*;
#[cfg(nexus_env = "os")]
pub use task::*;
#[cfg(nexus_env = "os")]
pub use time::*;
#[cfg(nexus_env = "os")]
pub use types::*;

// Root-level shared items the submodules reach through their `use super::*`.
#[cfg(nexus_env = "os")]
pub(crate) use crate::{IpcError, MsgHeader, Result};
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(crate) fn decode_syscall(value: usize) -> SysResult<usize> {
    if let Some(err) = AbiError::from_raw(value) {
        Err(err)
    } else {
        Ok(value)
    }
}

// ——— Architecture-specific ecall helpers (riscv64, OS) ———
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
pub(crate) unsafe fn ecall0(n: usize) -> usize {
    let mut r7 = n;
    let r0: usize;
    core::arch::asm!(
        "ecall",
        inout("a7") r7,
        lateout("a0") r0,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
pub(crate) unsafe fn ecall1(n: usize, a0: usize) -> usize {
    let mut r0 = a0;
    let mut r7 = n;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
pub(crate) unsafe fn ecall1_pair(n: usize, a0: usize) -> (usize, usize) {
    let mut r0 = a0;
    let mut r7 = n;
    let mut r1: usize;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        lateout("a1") r1,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    (r0, r1)
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
pub(crate) unsafe fn ecall2(n: usize, a0: usize, a1: usize) -> usize {
    let mut r0 = a0;
    let mut r1 = a1;
    let mut r7 = n;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        inout("a1") r1,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
pub(crate) unsafe fn ecall3(n: usize, a0: usize, a1: usize, a2: usize) -> usize {
    let mut r0 = a0;
    let mut r1 = a1;
    let mut r2 = a2;
    let mut r7 = n;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        inout("a1") r1,
        inout("a2") r2,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
pub(crate) unsafe fn ecall4(n: usize, a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    let mut r0 = a0;
    let mut r1 = a1;
    let mut r2 = a2;
    let mut r3 = a3;
    let mut r7 = n;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        inout("a1") r1,
        inout("a2") r2,
        inout("a3") r3,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
pub(crate) unsafe fn ecall5(
    n: usize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
) -> usize {
    let mut r0 = a0;
    let mut r1 = a1;
    let mut r2 = a2;
    let mut r3 = a3;
    let mut r4 = a4;
    let mut r7 = n;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        inout("a1") r1,
        inout("a2") r2,
        inout("a3") r3,
        inout("a4") r4,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
pub(crate) unsafe fn ecall6(
    n: usize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
) -> usize {
    let mut r0 = a0;
    let mut r1 = a1;
    let mut r2 = a2;
    let mut r3 = a3;
    let mut r4 = a4;
    let mut r5 = a5;
    let mut r7 = n;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        inout("a1") r1,
        inout("a2") r2,
        inout("a3") r3,
        inout("a4") r4,
        inout("a5") r5,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}
