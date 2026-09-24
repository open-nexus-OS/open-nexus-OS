// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: what a driver needs to point a bus master at memory (RFC-0098 C4,
//! TASK-0286 P4a). `vmo_runs` (syscall 60) is the one door a physical address
//! leaves the kernel by: the `(pa, len)` runs behind a byte range of a writable
//! VMO, adjacent runs merged, all or nothing, only to a task that holds a device
//! capability. `vmo_dma_base` is the one-run case for a `vmo_create_contiguous`
//! object (virtio queues, command pools). `cap_query` names no VMO base.
//! OWNERS: @runtime @drivers
//! STATUS: Functional
//! PUBLIC API: DmaRun, MAX_DMA_RUNS, vmo_runs(), vmo_dma_base()
//! TEST_COVERAGE: the kernel's `dma_runs` host tests (the reject matrix) and every
//!   DMA driver on every QEMU boot

#[cfg(nexus_env = "os")]
use super::*;

/// One physically contiguous run of a VMO, as `vmo_runs` writes it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DmaRun {
    /// Physical (bus) address of the run's first byte.
    pub pa: u64,
    /// Bytes in the run.
    pub len: u64,
}

/// The most runs one `vmo_runs` call answers (the kernel's `dma_runs::MAX_RUNS`).
pub const MAX_DMA_RUNS: usize = 256;

/// The runs behind `offset..offset + len` of the VMO in `slot`, into `out`;
/// returns how many were written. Refused (never truncated) when the range needs
/// more than `out.len()` runs, lies outside the object, names a read-only alias,
/// or the caller holds no device capability.
#[cfg(nexus_env = "os")]
pub fn vmo_runs(slot: Handle, offset: usize, len: usize, out: &mut [DmaRun]) -> SysResult<usize> {
    if out.is_empty() || out.len() > MAX_DMA_RUNS {
        return Err(AbiError::InvalidArgument);
    }
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_VMO_RUNS: usize = 60;
        // SAFETY: `out` is a live, writable buffer of `out.len()` `DmaRun`s
        // (16 bytes each, `repr(C)`: pa then len), exactly what the kernel writes.
        let raw = unsafe {
            ecall5(
                SYSCALL_VMO_RUNS,
                slot as usize,
                offset,
                len,
                out.as_mut_ptr() as usize,
                out.len(),
            )
        };
        decode_syscall(raw)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (slot, offset, len);
        Err(AbiError::Unsupported)
    }
}

/// The bus address of a physically contiguous VMO of `len` bytes (made with
/// `vmo_create_contiguous`): its one run. An object of more than one run is
/// refused — the caller asked for the wrong kind.
#[cfg(nexus_env = "os")]
pub fn vmo_dma_base(slot: Handle, len: usize) -> SysResult<u64> {
    let mut run = [DmaRun::default()];
    match vmo_runs(slot, 0, len, &mut run)? {
        1 if run[0].len == len as u64 => Ok(run[0].pa),
        _ => Err(AbiError::InvalidArgument),
    }
}
