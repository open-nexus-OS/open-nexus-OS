// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: what a driver needs to point a bus master at memory (RFC-0098 C4,
//! TASK-0286 P4a/P4b, device-scoped by TASK-0246 P1). A device reaches only what its
//! bus's `dma-ranges` name, and on a bus that translates the address it is programmed
//! with is not the physical one. init hands the kernel each device as ONE versioned
//! descriptor (`DeviceDesc`: register window, PLIC line, coherence, DMA reach). A
//! driver makes memory FOR its device (`vmo_create_for`, `vmo_create_contiguous`: the
//! frames come from inside the device's reach) and learns where the device sees it
//! through `vmo_runs` (syscall 60), the one door an address leaves the kernel by: the
//! `(bus, len)` runs behind a byte range of a writable VMO, in THAT device's bus
//! addresses, adjacent runs merged, all or nothing, only to the holder of the
//! device's capability. `vmo_dma_base` is the one-run case for a contiguous object
//! (virtio queues, command pools). `cap_query` names no VMO base.
//! Coherence travels with the device (P4b): its capability says whether it snoops
//! the CPU caches (`DmaCoherence`); for one that does not, the driver writes back
//! and drops its lines with Zicbom (`cache_clean` / `cache_flush`), which the
//! kernel enables for user mode — `cbo.inval` runs as a flush, so user mode can
//! never discard data. `DmaVmo` is a VMO mapped for DMA to one device (bytes +
//! runs), the memory `nexus_driverkit::DmaBuffer` moves between the CPU and it.
//! This module is the crate's second unsafe island next to the ecall helpers: the
//! `cbo` instructions and the byte view of a mapping this module made itself.
//! OWNERS: @runtime @drivers
//! STATUS: Functional
//! PUBLIC API: DmaRun, MAX_DMA_RUNS, vmo_runs(), vmo_dma_base(), DmaWindow, DeviceDesc,
//!   MAX_DMA_WINDOWS, DEVICE_DESC_VERSION, CAP_FLAG_DMA_NONCOHERENT, DmaCoherence,
//!   device_dma_coherence(), cache_clean(), cache_flush(), DmaVmo
//! TEST_COVERAGE: `tests` below (descriptor layout, coherence decoding); the kernel's
//!   `dma_reach` / `dma_runs` host tests (decode and reject matrix);
//!   `KSELFTEST: vmo reach ok (…)`; `SELFTEST: dma buffer ok (…)` runs the
//!   instructions through a `DmaBuffer` on every QEMU boot

#[cfg(nexus_env = "os")]
use super::*;

/// One run of a VMO as a device addresses it, as `vmo_runs` writes it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DmaRun {
    /// The address the device is programmed with for the run's first byte — its
    /// bus address; on a bus that translates, not the physical one.
    pub bus: u64,
    /// Bytes in the run.
    pub len: u64,
}

/// The most runs one `vmo_runs` call answers (the kernel's `dma_runs::MAX_RUNS`).
pub const MAX_DMA_RUNS: usize = 256;

/// The runs behind `offset..offset + len` of the VMO in `vmo`, as the device behind
/// the capability in `device` addresses them, into `out`; returns how many were
/// written. Refused (never truncated) when the range needs more than `out.len()`
/// runs, lies outside the object, names a read-only alias, reaches outside the
/// device's DMA reach, or `device` is not a device capability of the caller's.
#[cfg(nexus_env = "os")]
pub fn vmo_runs(
    vmo: Handle,
    device: Cap,
    offset: usize,
    len: usize,
    out: &mut [DmaRun],
) -> SysResult<usize> {
    if out.is_empty() || out.len() > MAX_DMA_RUNS {
        return Err(AbiError::InvalidArgument);
    }
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_VMO_RUNS: usize = 60;
        // SAFETY: `out` is a live, writable buffer of `out.len()` `DmaRun`s
        // (16 bytes each, `repr(C)`: bus then len), exactly what the kernel writes.
        let raw = unsafe {
            ecall6(
                SYSCALL_VMO_RUNS,
                vmo as usize,
                device as usize,
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
        let _ = (vmo, device, offset, len);
        Err(AbiError::Unsupported)
    }
}

/// The bus address `device` sees a physically contiguous VMO of `len` bytes at
/// (made with `vmo_create_contiguous` for it): its one run. An object of more than
/// one run is refused — the caller asked for the wrong kind.
#[cfg(nexus_env = "os")]
pub fn vmo_dma_base(vmo: Handle, device: Cap, len: usize) -> SysResult<u64> {
    let mut run = [DmaRun::default()];
    match vmo_runs(vmo, device, 0, len, &mut run)? {
        1 if run[0].len == len as u64 => Ok(run[0].bus),
        _ => Err(AbiError::InvalidArgument),
    }
}

/// Windows a device's DMA reach carries at most (the kernel's `MAX_DMA_WINDOWS`).
pub const MAX_DMA_WINDOWS: usize = 4;

/// The descriptor layout `device_mmio_cap_create` speaks.
pub const DEVICE_DESC_VERSION: u32 = 1;

/// One window of a device's DMA reach: device-visible `bus .. bus + size` is
/// physical `cpu .. cpu + size` (a bus's `dma-ranges`, composed up the tree).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DmaWindow {
    /// The window's first address as the device sees it.
    pub bus: u64,
    /// The physical address that bus address reaches.
    pub cpu: u64,
    /// Bytes in the window.
    pub size: u64,
}

/// A device as init hands it to the kernel (`device_mmio_cap_create`, RFC-0098
/// C3/C4): the register window, the PLIC line, whether it snoops the CPU caches and
/// what its DMA can reach — every fact from the device tree. Little endian,
/// `repr(C)`, 128 bytes; no window means the device reaches all memory, identity.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceDesc {
    /// `DEVICE_DESC_VERSION`.
    pub version: u32,
    /// Bit 0 (`CAP_FLAG_DMA_NONCOHERENT`): the device does not snoop the caches.
    pub flags: u32,
    /// The register window's physical base (page-aligned).
    pub base: u64,
    /// The register window's bytes (whole pages).
    pub len: u64,
    /// The PLIC line (0 = none).
    pub irq: u32,
    /// How many of `dma` are the reach; the rest are zero.
    pub dma_count: u32,
    /// The DMA reach's windows (none: all memory, identity).
    pub dma: [DmaWindow; MAX_DMA_WINDOWS],
}

// The layout the kernel decodes (`dma_reach::decode_desc`), proven here, not assumed.
const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(size_of::<DeviceDesc>() == 32 + 24 * MAX_DMA_WINDOWS);
    assert!(offset_of!(DeviceDesc, flags) == 4 && offset_of!(DeviceDesc, base) == 8);
    assert!(offset_of!(DeviceDesc, len) == 16 && offset_of!(DeviceDesc, irq) == 24);
    assert!(offset_of!(DeviceDesc, dma_count) == 28 && offset_of!(DeviceDesc, dma) == 32);
    assert!(size_of::<DmaWindow>() == 24 && offset_of!(DmaWindow, cpu) == 8);
    assert!(offset_of!(DmaWindow, size) == 16 && cfg!(target_endian = "little"));
};

impl DeviceDesc {
    /// A descriptor; `None` when the reach has more windows than the layout carries.
    pub fn new(
        base: u64,
        len: u64,
        irq: u32,
        noncoherent: bool,
        reach: &[DmaWindow],
    ) -> Option<Self> {
        let mut dma = [DmaWindow::default(); MAX_DMA_WINDOWS];
        dma.get_mut(..reach.len())?.copy_from_slice(reach);
        Some(Self {
            version: DEVICE_DESC_VERSION,
            flags: if noncoherent { CAP_FLAG_DMA_NONCOHERENT } else { 0 },
            base,
            len,
            irq,
            dma_count: reach.len() as u32,
            dma,
        })
    }
}

/// `CapQuery::flags` bit 0: the device does not snoop the CPU caches.
pub const CAP_FLAG_DMA_NONCOHERENT: u32 = 1;

/// How a device sees memory the CPU wrote (RFC-0098 C4, TASK-0286 P4b).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaCoherence {
    /// It snoops the CPU caches: no maintenance.
    Coherent,
    /// It does not: the driver writes back and drops lines in `block`-byte cache
    /// blocks around every transfer (Zicbom).
    Maintained {
        /// The harts' Zicbom cache-block size in bytes.
        block: usize,
    },
    /// It does not, and the harts have no Zicbom: no buffer can be made safe for it.
    Unmaintainable,
}

impl DmaCoherence {
    /// From a device capability's query (`CapQuery::flags`, `CapQuery::cache_block`).
    pub fn from_device(flags: u32, cache_block: u32) -> Self {
        let block = cache_block as usize;
        if flags & CAP_FLAG_DMA_NONCOHERENT == 0 {
            Self::Coherent
        } else if block.is_power_of_two() && (16..=4096).contains(&block) {
            Self::Maintained { block }
        } else {
            Self::Unmaintainable
        }
    }
}

/// The coherence of the device behind the capability in `device` — refused for a
/// slot that holds no device.
#[cfg(nexus_env = "os")]
pub fn device_dma_coherence(device: Cap) -> SysResult<DmaCoherence> {
    let mut query = CapQuery::default();
    cap_query(device, &mut query)?;
    if query.kind_tag != 2 {
        return Err(AbiError::InvalidArgument);
    }
    Ok(DmaCoherence::from_device(query.flags, query.cache_block))
}

#[derive(Clone, Copy)]
enum Cbo {
    Clean,
    Flush,
}

/// Write back every cache block `bytes` touches (`cbo.clean`), then fence — before a
/// device reads memory the CPU wrote. Neighbouring bytes in the first and last block
/// are written back too; their contents never change. A no-op off RISC-V hardware.
pub fn cache_clean(bytes: &[u8], block: usize) {
    cbo_range(Cbo::Clean, bytes, block);
}

/// Write back and drop every cache block `bytes` touches (`cbo.flush`), then fence —
/// before a device writes the memory, and again before the CPU reads what it wrote.
pub fn cache_flush(bytes: &[u8], block: usize) {
    cbo_range(Cbo::Flush, bytes, block);
}

fn cbo_range(op: Cbo, bytes: &[u8], block: usize) {
    if bytes.is_empty() || !block.is_power_of_two() || block > 4096 {
        return;
    }
    let start = bytes.as_ptr() as usize & !(block - 1);
    let end = bytes.as_ptr() as usize + bytes.len();
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        let mut at = start;
        while at < end {
            // SAFETY: `at` lies in a cache block that overlaps `bytes`; a block never
            // crosses a page (block ≤ 4096, both powers of two), so the page is the
            // caller's mapping of `bytes`. Clean/flush change no memory contents.
            unsafe {
                match op {
                    Cbo::Clean => core::arch::asm!(
                        ".option push",
                        ".option arch, +zicbom",
                        "cbo.clean ({0})",
                        ".option pop",
                        in(reg) at,
                        options(nostack)
                    ),
                    Cbo::Flush => core::arch::asm!(
                        ".option push",
                        ".option arch, +zicbom",
                        "cbo.flush ({0})",
                        ".option pop",
                        in(reg) at,
                        options(nostack)
                    ),
                }
            }
            at += block;
        }
        // Order the maintenance before the doorbell (and every access) that follows.
        // SAFETY: a fence has no operands.
        unsafe { core::arch::asm!("fence iorw, iorw", options(nostack)) };
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (op, start, end);
    }
}

/// Runs a `DmaVmo` records (a larger object is refused: a device list longer than
/// this is a fragmented anonymous object, ask for `contiguous` instead).
pub const DMA_VMO_MAX_RUNS: usize = 16;

/// A VMO mapped for DMA to one device: the bytes the CPU fills or reads and the runs
/// the device is programmed with. Its frames come from inside the device's reach,
/// and only the holder of the device's capability can make one. Fresh memory is zero
/// (the kernel zeroes every object at create); drop unmaps and destroys it.
#[cfg(nexus_env = "os")]
pub struct DmaVmo {
    vmo: Handle,
    va: usize,
    map_len: usize,
    len: usize,
    runs: [DmaRun; DMA_VMO_MAX_RUNS],
    count: usize,
}

#[cfg(nexus_env = "os")]
impl DmaVmo {
    /// One physically contiguous run for `device` (a device that takes one base).
    pub fn contiguous(device: Cap, len: usize) -> SysResult<Self> {
        Self::create(device, len, true)
    }

    /// Any runs for `device` (a device that takes a scatter-gather list).
    pub fn anonymous(device: Cap, len: usize) -> SysResult<Self> {
        Self::create(device, len, false)
    }

    fn create(device: Cap, len: usize, contiguous: bool) -> SysResult<Self> {
        if len == 0 {
            return Err(AbiError::InvalidArgument);
        }
        let map_len = len.div_ceil(4096) * 4096;
        let vmo = if contiguous {
            vmo_create_contiguous(device, map_len)
        } else {
            vmo_create_for(device, map_len)
        }
        .map_err(|_| AbiError::OutOfMemory)?;
        let flags = page_flags::VALID | page_flags::USER | page_flags::READ | page_flags::WRITE;
        let va = match vm_map(vmo, 0, map_len, flags) {
            Ok(va) => va,
            Err(err) => {
                let _ = vmo_destroy(vmo);
                return Err(err);
            }
        };
        let mut runs = [DmaRun::default(); DMA_VMO_MAX_RUNS];
        match vmo_runs(vmo, device, 0, len, &mut runs) {
            Ok(count) => Ok(Self { vmo, va, map_len, len, runs, count }),
            Err(err) => {
                let _ = vm_unmap(va, map_len);
                let _ = vmo_destroy(vmo);
                Err(err)
            }
        }
    }

    /// The VMO (to hand to another service, e.g. with `cap_clone`).
    pub fn handle(&self) -> Handle {
        self.vmo
    }

    /// Bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Never true: an empty object is refused at create.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// What the device is programmed with (its bus addresses).
    pub fn runs(&self) -> &[DmaRun] {
        &self.runs[..self.count]
    }

    /// The CPU's view.
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: `va..va + len` is this object's own read-write mapping, alive until
        // drop; the borrow ties the slice to `self`.
        unsafe { core::slice::from_raw_parts(self.va as *const u8, self.len) }
    }

    /// The CPU's view, writable.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as `bytes`, and `&mut self` makes the view exclusive.
        unsafe { core::slice::from_raw_parts_mut(self.va as *mut u8, self.len) }
    }
}

#[cfg(nexus_env = "os")]
impl Drop for DmaVmo {
    fn drop(&mut self) {
        let _ = vm_unmap(self.va, self.map_len);
        let _ = vmo_destroy(self.vmo);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_descriptor_carries_every_fact_and_zeroes_the_unused_windows() {
        let w = DmaWindow { bus: 0x8000_0000, cpu: 0x1_0000_0000, size: 0x8000_0000 };
        let desc = DeviceDesc::new(0xd428_1000, 0x1000, 101, true, &[w]).unwrap();
        assert_eq!((desc.version, desc.flags), (DEVICE_DESC_VERSION, CAP_FLAG_DMA_NONCOHERENT));
        assert_eq!((desc.base, desc.len, desc.irq, desc.dma_count), (0xd428_1000, 0x1000, 101, 1));
        assert_eq!(desc.dma[0], w);
        assert!(desc.dma[1..].iter().all(|u| *u == DmaWindow::default()));
        let coherent = DeviceDesc::new(0xd428_1000, 0x1000, 0, false, &[]).unwrap();
        assert_eq!((coherent.flags, coherent.dma_count), (0, 0), "no window: all memory");
    }

    #[test]
    fn test_reject_a_reach_wider_than_the_descriptor() {
        let w = DmaWindow { bus: 0, cpu: 0, size: 0x1000 };
        assert!(DeviceDesc::new(0, 0x1000, 0, false, &[w; MAX_DMA_WINDOWS]).is_some());
        assert_eq!(DeviceDesc::new(0, 0x1000, 0, false, &[w; MAX_DMA_WINDOWS + 1]), None);
    }

    #[test]
    fn a_coherent_device_needs_nothing_whatever_the_harts_have() {
        assert_eq!(DmaCoherence::from_device(0, 64), DmaCoherence::Coherent);
        assert_eq!(DmaCoherence::from_device(0, 0), DmaCoherence::Coherent);
    }

    #[test]
    fn a_non_coherent_device_is_maintained_in_the_harts_block() {
        let flags = CAP_FLAG_DMA_NONCOHERENT;
        assert_eq!(DmaCoherence::from_device(flags, 64), DmaCoherence::Maintained { block: 64 });
    }

    #[test]
    fn test_reject_maintenance_without_zicbom_or_with_a_corrupt_block() {
        let flags = CAP_FLAG_DMA_NONCOHERENT;
        assert_eq!(DmaCoherence::from_device(flags, 0), DmaCoherence::Unmaintainable);
        assert_eq!(DmaCoherence::from_device(flags, 48), DmaCoherence::Unmaintainable);
        assert_eq!(DmaCoherence::from_device(flags, 8), DmaCoherence::Unmaintainable);
        assert_eq!(DmaCoherence::from_device(flags, 8192), DmaCoherence::Unmaintainable);
    }

    #[test]
    fn maintenance_off_hardware_touches_nothing() {
        let bytes = [7u8; 100];
        cache_clean(&bytes, 64);
        cache_flush(&bytes, 64);
        cache_flush(&[], 64);
        cache_clean(&bytes, 3);
        assert_eq!(bytes, [7u8; 100]);
    }
}
