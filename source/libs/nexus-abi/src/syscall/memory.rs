// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Memory syscalls — address spaces, VMOs, page flags, MMIO mapping, cap queries
//! (Mechanical split out of the former lib.rs monolith — ADR-0051 hygiene
//! pass; behavior and syscall IDs unchanged.)

#[cfg(nexus_env = "os")]
use super::*;
/// C (Phase C): returns the caller's own address-space handle (raw).
#[cfg(nexus_env = "os")]
#[must_use = "as_self result must be handled"]
pub fn as_self() -> SysResult<u32> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_AS_SELF: usize = 49;
        let raw = unsafe { ecall0(SYSCALL_AS_SELF) };
        decode_syscall(raw).map(|v| v as u32)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Drops the caller's reference to an address space handle.
#[cfg(nexus_env = "os")]
pub fn as_destroy(handle: AsHandle) -> SysResult<()> {
    let _ = handle;
    Err(AbiError::Unsupported)
}

/// Allocates a new address space and returns its opaque handle.
#[cfg(nexus_env = "os")]
pub fn as_create() -> SysResult<AsHandle> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_AS_CREATE: usize = 9;
        let raw = unsafe { ecall0(SYSCALL_AS_CREATE) };
        decode_syscall(raw).map(|handle| handle as AsHandle)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Maps a VMO into the target address space referenced by `as_handle`.
#[cfg(nexus_env = "os")]
pub fn as_map(
    as_handle: AsHandle,
    vmo: Handle,
    va: u64,
    len: u64,
    prot: u32,
    flags: u32,
) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_AS_MAP: usize = 10;
        if va > usize::MAX as u64 || len > usize::MAX as u64 {
            return Err(AbiError::Unsupported);
        }
        let raw = unsafe {
            ecall6(
                SYSCALL_AS_MAP,
                as_handle as usize,
                vmo as usize,
                va as usize,
                len as usize,
                prot as usize,
                flags as usize,
            )
        };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

// ——— VMO userland wrappers (OS build) ———

/// `vmo_create` arg 3: the object is for no device.
#[cfg(nexus_env = "os")]
const VMO_NO_DEVICE: usize = usize::MAX;

/// Creates a new anonymous VMO of `len` bytes (page-backed: a list of
/// physically contiguous runs, no single physical base) and returns a handle.
#[cfg(nexus_env = "os")]
pub fn vmo_create(_len: usize) -> Result<Handle> {
    vmo_create_kind(_len, 0, VMO_NO_DEVICE)
}

/// RFC-0098 C4 (TASK-0246 P1): like [`vmo_create`], but the object is FOR the device
/// behind the capability in `device` — its frames come from inside that device's DMA
/// reach, so every run has a bus address for it ([`vmo_runs`]). Memory a device
/// reads through a scatter-gather list.
#[cfg(nexus_env = "os")]
pub fn vmo_create_for(device: Cap, len: usize) -> Result<Handle> {
    vmo_create_kind(len, 0, device as usize)
}

/// RFC-0098 C4: an object for `device` that is ONE physically contiguous block —
/// the kind for memory a device addresses by one base (virtio queues, command and
/// response pools, request buffers); its bus address is [`vmo_dma_base`]. The
/// kernel refuses it without a device capability (TASK-0246 P1).
#[cfg(nexus_env = "os")]
pub fn vmo_create_contiguous(device: Cap, len: usize) -> Result<Handle> {
    vmo_create_kind(len, 1, device as usize)
}

#[cfg(nexus_env = "os")]
fn vmo_create_kind(_len: usize, _kind: usize, _device: usize) -> Result<Handle> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        const SYSCALL_VMO_CREATE: usize = 5;
        let slot = usize::MAX;
        let raw = ecall4(SYSCALL_VMO_CREATE, slot, _len, _kind, _device);
        match decode_syscall(raw) {
            Ok(slot) => Ok(slot as Handle),
            Err(_) => Err(IpcError::Unsupported),
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(IpcError::Unsupported)
    }
}

/// Writes `bytes` into the VMO starting at `offset` bytes from the base.
#[cfg(nexus_env = "os")]
pub fn vmo_write(_handle: Handle, _offset: usize, _bytes: &[u8]) -> Result<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        const SYSCALL_VMO_WRITE: usize = 6;
        let len = _bytes.len();
        let ptr = _bytes.as_ptr() as usize;
        let raw = ecall4(SYSCALL_VMO_WRITE, _handle as usize, _offset, ptr, len);
        match decode_syscall(raw) {
            Ok(_) => Ok(()),
            Err(_) => Err(IpcError::Unsupported),
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(IpcError::Unsupported)
    }
}

/// Reads bytes out of the VMO starting at `offset` into `buf` — the mirror of
/// [`vmo_write`] (syscall 47). The ADR-0042 compositor damage-blit is the
/// first consumer: windowd reads app-surface pixels through this (userspace
/// has no VMO mapping path).
#[cfg(nexus_env = "os")]
pub fn vmo_read(_handle: Handle, _offset: usize, _buf: &mut [u8]) -> Result<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        const SYSCALL_VMO_READ: usize = 47;
        let len = _buf.len();
        let ptr = _buf.as_mut_ptr() as usize;
        let raw = ecall4(SYSCALL_VMO_READ, _handle as usize, _offset, ptr, len);
        match decode_syscall(raw) {
            Ok(_) => Ok(()),
            Err(_) => Err(IpcError::Unsupported),
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(IpcError::Unsupported)
    }
}

/// Page-table leaf flags for user mappings (Sv39).
///
/// These constants match `source/kernel/neuron/src/mm/page_table.rs` `PageFlags` bits.
#[cfg(nexus_env = "os")]
pub mod page_flags {
    /// Entry is valid.
    pub const VALID: u32 = 1 << 0;
    /// Readable.
    pub const READ: u32 = 1 << 1;
    /// Writable.
    pub const WRITE: u32 = 1 << 2;
    /// Executable.
    pub const EXECUTE: u32 = 1 << 3;
    /// User accessible.
    pub const USER: u32 = 1 << 4;
}

/// RFC-0085: maps a whole VMO range at a KERNEL-CHOSEN virtual address and
/// returns that address. The kernel allocates inside its managed user window
/// (first-fit, superpage-phase-aware for ≥2 MiB ranges); userspace never
/// invents the address. `offset`/`len` are byte values, page-aligned;
/// `flags` uses `page_flags::*` bits (EXECUTE is refused).
#[cfg(nexus_env = "os")]
pub fn vm_map(_handle: Handle, _offset: usize, _len: usize, _flags: u32) -> SysResult<usize> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_VM_MAP: usize = 53;
        let raw =
            unsafe { ecall4(SYSCALL_VM_MAP, _handle as usize, _offset, _len, _flags as usize) };
        decode_syscall(raw)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_handle, _offset, _len, _flags);
        Err(AbiError::Unsupported)
    }
}

/// RFC-0085: unmaps ONE exact region previously returned by [`vm_map`] /
/// [`mmio_map_auto`] — whole region only (no splitting in v1). Errors keep
/// their identity: `NotFound` (nothing there), `InvalidArgument` (length
/// mismatch), `CapabilityDenied` (kernel-placed Fixed region).
#[cfg(nexus_env = "os")]
pub fn vm_unmap(_va: usize, _len: usize) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_VM_UNMAP: usize = 54;
        let raw = unsafe { ecall2(SYSCALL_VM_UNMAP, _va, _len) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_va, _len);
        Err(AbiError::Unsupported)
    }
}

/// RFC-0085: maps a device-MMIO window at a KERNEL-CHOSEN virtual address
/// and returns it. Security floor: USER|RW, never EXEC. Idempotency by
/// abolition: a caller that never chooses an address cannot collide.
#[cfg(nexus_env = "os")]
pub fn mmio_map_auto(_handle: Handle, _offset: usize, _len: usize) -> SysResult<usize> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_MMIO_MAP_AUTO: usize = 55;
        let raw = unsafe { ecall3(SYSCALL_MMIO_MAP_AUTO, _handle as usize, _offset, _len) };
        decode_syscall(raw)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_handle, _offset, _len);
        Err(AbiError::Unsupported)
    }
}

/// Information about an address-bearing capability (VMO or device MMIO window).
#[cfg(nexus_env = "os")]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CapQuery {
    /// 1 = VMO, 2 = DeviceMmio, 3 = read-only VMO alias.
    pub kind_tag: u32,
    /// DeviceMmio: the device's PLIC line from the device tree's `interrupts`
    /// (0 = none) — the one place a driver learns its interrupt (RFC-0098 C3).
    /// 0 for every other kind.
    pub irq: u32,
    /// DeviceMmio: the physical base of the register window. 0 for a VMO — a
    /// VMO's physical runs leave the kernel only through [`vmo_runs`] /
    /// [`vmo_dma_base`] (RFC-0098 C4, TASK-0286 P4a).
    pub base: u64,
    /// Length in bytes of the capability's window.
    pub len: u64,
    /// DeviceMmio: bit 0 ([`CAP_FLAG_DMA_NONCOHERENT`]) = the device does not snoop
    /// the CPU caches — init read it from the tree like the line (RFC-0098 C4).
    pub flags: u32,
    /// DeviceMmio: the harts' Zicbom cache-block size in bytes, 0 = no Zicbom —
    /// what a driver of a non-coherent device maintains in ([`DmaCoherence`]).
    pub cache_block: u32,
}

/// The memory record `mm_stats` answers (RFC-0098 C4, TASK-0286 P5): the frame
/// pool, the objects, and the caller's own residency — nothing about another task.
/// Layout = the kernel's `accounting` wire form (version, field count, 64-bit fields).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MmStats {
    /// Layout version (`MM_STATS_VERSION`).
    pub version: u32,
    /// 64-bit fields that follow.
    pub fields: u32,
    /// Memory banks the tree named.
    pub banks: u64,
    /// Frames handed to the pool.
    pub total: u64,
    /// Frames free now.
    pub free: u64,
    /// Frames the tree reserved.
    pub reserved: u64,
    /// Frames the kernel excluded (its image, the tree).
    pub excluded: u64,
    /// Blocks allocated since boot.
    pub allocs: u64,
    /// Blocks returned since boot.
    pub frees: u64,
    /// Requests the pool could not satisfy at all.
    pub exhausted: u64,
    /// Frames holding page tables.
    pub pt_frames: u64,
    /// Live VMOs.
    pub vmos: u64,
    /// Bytes they hold.
    pub vmo_bytes: u64,
    /// Bytes held by contiguous (DMA) VMOs.
    pub dma_bytes: u64,
    /// The caller's resident bytes.
    pub own_rss_bytes: u64,
    /// The part of those that is contiguous-DMA memory.
    pub own_dma_bytes: u64,
}

/// The record layout this ABI speaks.
pub const MM_STATS_VERSION: u32 = 1;
const _: () = assert!(core::mem::size_of::<MmStats>() == 8 + 14 * 8);

/// Frame size of the pool (`MmStats::total` / `free` count these).
pub const FRAME_BYTES: u64 = 4096;

/// The memory record (syscall 61); refused unless the kernel speaks this layout.
#[cfg(nexus_env = "os")]
pub fn mm_stats() -> SysResult<MmStats> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_MM_STATS: usize = 61;
        let mut stats = MmStats::default();
        let len = core::mem::size_of::<MmStats>();
        // SAFETY: `stats` is a live, writable `repr(C)` record of `len` bytes.
        let raw = unsafe { ecall2(SYSCALL_MM_STATS, (&mut stats as *mut MmStats) as usize, len) };
        decode_syscall(raw)?;
        if stats.version != MM_STATS_VERSION || stats.fields != 14 {
            return Err(AbiError::Unsupported);
        }
        Ok(stats)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Queries a capability slot and writes the result into `out`.
#[cfg(nexus_env = "os")]
pub fn cap_query(_cap: Cap, _out: &mut CapQuery) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_CAP_QUERY: usize = 28;
        let out_ptr = (_out as *mut CapQuery) as usize;
        let raw = unsafe { ecall2(SYSCALL_CAP_QUERY, _cap as usize, out_ptr) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_cap, _out);
        Err(AbiError::Unsupported)
    }
}

/// The PLIC line a device capability carries (RFC-0098 C3: init took it from the
/// node's `interrupts`); 0 when the slot is not a device or lists no line.
#[cfg(nexus_env = "os")]
#[must_use]
pub fn device_irq(slot: Cap) -> u32 {
    let mut info = CapQuery::default();
    match cap_query(slot, &mut info) {
        Ok(()) if info.kind_tag == 2 => info.irq,
        _ => 0,
    }
}

/// Creates a DeviceMmio capability in the caller's cap table (init-only) from the
/// device's descriptor — register window, PLIC line (0 = none), coherence and DMA
/// reach, all from the device tree's node and the buses above it (RFC-0098 C3/C4).
/// The kernel keeps one record per register window: describing a window again
/// identically names the same record; differently is refused.
///
/// If `slot_raw` is `usize::MAX`, the kernel allocates a fresh slot; otherwise, the cap is placed
/// into the requested slot (must be empty).
#[cfg(nexus_env = "os")]
pub fn device_mmio_cap_create(_desc: &DeviceDesc, _slot_raw: usize) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_DEVICE_CAP_CREATE: usize = 30;
        let ptr = (_desc as *const DeviceDesc) as usize;
        let len = core::mem::size_of::<DeviceDesc>();
        // SAFETY: `_desc` is a live `repr(C)` descriptor of `len` bytes the kernel
        // only reads.
        let raw = unsafe { ecall3(SYSCALL_DEVICE_CAP_CREATE, ptr, len, _slot_raw) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_desc, _slot_raw);
        Err(AbiError::Unsupported)
    }
}

/// Releases a sole-owned VMO back to the kernel arena (task #124).
///
/// For self-created, never-shared one-shot VMOs (staging buffers, the boot-splash
/// backing). The kernel refuses while any other capability in the system still
/// references the range. The caller must not touch the memory afterwards —
/// including through mappings it made with `vm_map`.
#[cfg(nexus_env = "os")]
pub fn vmo_destroy(handle: Handle) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_VMO_DESTROY: usize = 46;
        let raw = unsafe { ecall1(SYSCALL_VMO_DESTROY, handle as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = handle;
        Err(AbiError::Unsupported)
    }
}

/// RFC-0080: derives a READ-ONLY alias of the VMO in `slot` and returns its
/// new cap slot. Holders of the alias can only map it read-only (WRITE|EXECUTE
/// are stripped by the kernel) and cannot `vmo_write` it — used to share the
/// glyph atlas so no app-host can corrupt the pages the others read.
#[cfg(nexus_env = "os")]
pub fn vmo_share_readonly(slot: Handle) -> SysResult<Handle> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_VMO_SHARE_RO: usize = 51;
        let raw = unsafe { ecall1(SYSCALL_VMO_SHARE_RO, slot as usize) };
        decode_syscall(raw).map(|s| s as Handle)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = slot;
        Err(AbiError::Unsupported)
    }
}
