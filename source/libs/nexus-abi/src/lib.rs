// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]

//! Shared ABI definitions exposed to userland crates.

use core::convert::TryInto;

/// Result type returned by ABI helpers.
pub type Result<T> = core::result::Result<T, IpcError>;

/// Errors surfaced by IPC syscalls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcError {
    /// Referenced endpoint is not present in the router.
    NoSuchEndpoint,
    /// Target queue ran out of space.
    QueueFull,
    /// Queue did not contain a message when operating in non-blocking mode.
    QueueEmpty,
    /// Caller lacks permission to perform the requested operation.
    PermissionDenied,
    /// IPC is not supported for this configuration.
    Unsupported,
}

/// IPC message header shared between kernel and userland.
#[repr(C, align(4))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsgHeader {
    /// Source capability slot.
    pub src: u32,
    /// Destination endpoint identifier.
    pub dst: u32,
    /// Message opcode.
    pub ty: u16,
    /// Transport flags.
    pub flags: u16,
    /// Inline payload length.
    pub len: u32,
}

impl MsgHeader {
    /// Creates a new header with the provided fields.
    pub const fn new(src: u32, dst: u32, ty: u16, flags: u16, len: u32) -> Self {
        Self {
            src,
            dst,
            ty,
            flags,
            len,
        }
    }

    /// Serialises the header to a little-endian byte array.
    pub fn to_le_bytes(&self) -> [u8; 16] {
        let mut buf = [0_u8; 16];
        buf[0..4].copy_from_slice(&self.src.to_le_bytes());
        buf[4..8].copy_from_slice(&self.dst.to_le_bytes());
        buf[8..10].copy_from_slice(&self.ty.to_le_bytes());
        buf[10..12].copy_from_slice(&self.flags.to_le_bytes());
        buf[12..16].copy_from_slice(&self.len.to_le_bytes());
        buf
    }

    /// Deserialises a little-endian byte array into a header.
    pub fn from_le_bytes(bytes: [u8; 16]) -> Self {
        let src = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let dst = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let ty = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        let flags = u16::from_le_bytes(bytes[10..12].try_into().unwrap());
        let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        Self {
            src,
            dst,
            ty,
            flags,
            len,
        }
    }
}

// ——— VMO userland wrappers (OS build) ———

/// Opaque handle identifying a Virtual Memory Object (VMO) in the kernel.
#[cfg(nexus_env = "os")]
pub type Handle = u32;

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
        let ret = ecall3(SYSCALL_VMO_CREATE, slot, len, 0);
        Ok(ret as Handle)
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
        let _ = ecall3(SYSCALL_VMO_WRITE, _handle as usize, _offset, len);
        Ok(())
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
        let _ = ecall4(SYSCALL_MAP, _handle as usize, _va, 0, _flags as usize);
        Ok(())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(IpcError::Unsupported)
    }
}

// ——— Architecture-specific ecall helpers (riscv64, OS) ———
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[inline(always)]
unsafe fn ecall3(n: usize, a0: usize, a1: usize, a2: usize) -> usize {
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
        options(nostack, preserves_flags)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[inline(always)]
unsafe fn ecall4(n: usize, a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
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
        options(nostack, preserves_flags)
    );
    r0
}

#[cfg(test)]
mod tests {
    use super::MsgHeader;
    use core::mem::{align_of, size_of};

    #[test]
    fn header_layout() {
        assert_eq!(size_of::<MsgHeader>(), 16);
        assert_eq!(align_of::<MsgHeader>(), 4);
    }

    #[test]
    fn round_trip() {
        let header = MsgHeader::new(1, 2, 3, 4, 5);
        assert_eq!(header, MsgHeader::from_le_bytes(header.to_le_bytes()));
    }
}
