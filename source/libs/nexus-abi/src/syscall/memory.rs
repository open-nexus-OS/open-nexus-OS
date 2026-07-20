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

/// Creates a new contiguous VMO of `len` bytes and returns a handle to it.
///
/// The initial implementation is a placeholder; the kernel syscall path will
/// be wired in a subsequent change.
#[cfg(nexus_env = "os")]
pub fn vmo_create(_len: usize) -> Result<Handle> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        const SYSCALL_VMO_CREATE: usize = 5;
        let slot = usize::MAX;
        let len = _len;
        let raw = ecall3(SYSCALL_VMO_CREATE, slot, len, 0);
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

/// Maps the VMO into the caller's address space at virtual address `va` with
/// the requested flags. The mapping is read-only in the initial path.
#[cfg(nexus_env = "os")]
pub fn vmo_map(_handle: Handle, _va: usize, _flags: u32) -> Result<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        const SYSCALL_MAP: usize = 4;
        // Offset=0 for the minimal path; flags passed as fourth arg.
        let raw = ecall4(SYSCALL_MAP, _handle as usize, _va, 0, _flags as usize);
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

/// Maps one page of a VMO into the caller's address space at virtual address `va`.
///
/// - `va` must be 4096-byte aligned.
/// - `offset` is a byte offset into the VMO (page-aligned by the kernel).
/// - `flags` uses `page_flags::*` bits.
#[cfg(nexus_env = "os")]
pub fn vmo_map_page(_handle: Handle, _va: usize, _offset: usize, _flags: u32) -> Result<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        const SYSCALL_MAP: usize = 4;
        let raw = ecall4(SYSCALL_MAP, _handle as usize, _va, _offset, _flags as usize);
        match decode_syscall(raw) {
            Ok(_) => Ok(()),
            Err(_) => Err(IpcError::Unsupported),
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_handle, _va, _offset, _flags);
        Err(IpcError::Unsupported)
    }
}

/// Like [`vmo_map_page`] but returns the raw syscall error (`AbiError`) for diagnostics.
#[cfg(nexus_env = "os")]
pub fn vmo_map_page_sys(_handle: Handle, _va: usize, _offset: usize, _flags: u32) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_MAP: usize = 4;
        let raw = unsafe { ecall4(SYSCALL_MAP, _handle as usize, _va, _offset, _flags as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_handle, _va, _offset, _flags);
        Err(AbiError::Unsupported)
    }
}

/// Maps a device MMIO window capability into the caller's address space at virtual address `va`.
///
/// Security invariants (enforced by kernel):
/// - mapping is USER + RW
/// - mapping is never executable
/// - mapping is bounded to the capability window
#[cfg(nexus_env = "os")]
pub fn mmio_map(_handle: Handle, _va: usize, _offset: usize) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_MMIO_MAP: usize = 27;
        let raw = unsafe { ecall3(SYSCALL_MMIO_MAP, _handle as usize, _va, _offset) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_handle, _va, _offset);
        Err(AbiError::Unsupported)
    }
}

/// Information about an address-bearing capability (VMO or device MMIO window).
#[cfg(nexus_env = "os")]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapQuery {
    /// 1 = VMO, 2 = DeviceMmio.
    pub kind_tag: u32,
    /// Reserved for future expansion (must be zero).
    pub reserved: u32,
    /// Physical base address for the capability's window.
    pub base: u64,
    /// Length in bytes of the capability's window.
    pub len: u64,
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

/// Creates a DeviceMmio capability in the caller's cap table (init-only).
///
/// If `slot_raw` is `usize::MAX`, the kernel allocates a fresh slot; otherwise, the cap is placed
/// into the requested slot (must be empty).
#[cfg(nexus_env = "os")]
pub fn device_mmio_cap_create(_base: usize, _len: usize, _slot_raw: usize) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_DEVICE_CAP_CREATE: usize = 30;
        let raw = unsafe { ecall3(SYSCALL_DEVICE_CAP_CREATE, _base, _len, _slot_raw) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_base, _len, _slot_raw);
        Err(AbiError::Unsupported)
    }
}

/// Releases a sole-owned VMO back to the kernel arena (task #124).
///
/// For self-created, never-shared one-shot VMOs (staging buffers, the boot-splash
/// backing). The kernel refuses while any other capability in the system still
/// references the range. The caller must not touch the memory afterwards —
/// including through mappings it made with `vmo_map_page`.
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
