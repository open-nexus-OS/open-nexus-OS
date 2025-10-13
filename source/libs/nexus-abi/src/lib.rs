// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![cfg_attr(
    not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
    forbid(unsafe_code)
)]
#![deny(clippy::all, missing_docs)]

//! Shared ABI definitions exposed to userland crates.

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
        let src = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let dst = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let ty = u16::from_le_bytes([bytes[8], bytes[9]]);
        let flags = u16::from_le_bytes([bytes[10], bytes[11]]);
        let len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        Self {
            src,
            dst,
            ty,
            flags,
            len,
        }
    }
}

// ——— Task and capability primitives (OS build) ———

#[cfg(nexus_env = "os")]
bitflags::bitflags! {
    /// Rights mask accepted by capability-transfer syscalls.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Rights: u32 {
        /// Permit the holder to send messages through the endpoint.
        const SEND = 1 << 0;
        /// Permit the holder to receive messages from the endpoint.
        const RECV = 1 << 1;
        /// Permit the holder to map VMOs into its address space.
        const MAP = 1 << 2;
        /// Permit the holder to manage capabilities (reserved for kernel tests).
        const MANAGE = 1 << 3;
    }
}

/// Kernel task identifier returned from [`spawn`].
#[cfg(nexus_env = "os")]
pub type Pid = u32;

/// Capability slot handle returned from [`cap_transfer`].
#[cfg(nexus_env = "os")]
pub type Cap = u32;

/// Result returned by privileged syscalls that expose kernel operations.
#[cfg(nexus_env = "os")]
pub type SysResult<T> = core::result::Result<T, AbiError>;

/// Errors surfaced when invoking privileged syscalls from userland.
#[cfg(nexus_env = "os")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbiError {
    /// Syscall number is not implemented by the kernel build.
    InvalidSyscall,
    /// Kernel rejected the request due to missing rights or invalid slots.
    CapabilityDenied,
    /// Kernel-side IPC machinery reported a routing error.
    IpcFailure,
    /// Kernel rejected process creation.
    SpawnFailed,
    /// Kernel rejected capability transfer.
    TransferFailed,
    /// Operation unsupported on the current build target.
    Unsupported,
}

#[cfg(nexus_env = "os")]
impl AbiError {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    fn from_raw(value: usize) -> Option<Self> {
        match value {
            usize::MAX => Some(Self::InvalidSyscall),
            val if val == usize::MAX - 1 => Some(Self::CapabilityDenied),
            val if val == usize::MAX - 2 => Some(Self::IpcFailure),
            val if val == usize::MAX - 3 => Some(Self::SpawnFailed),
            val if val == usize::MAX - 4 => Some(Self::TransferFailed),
            _ => None,
        }
    }
}

// ——— Syscall wrappers (OS build) ———

/// Cooperative yield hint to the scheduler.
#[cfg(nexus_env = "os")]
pub fn yield_() -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_YIELD: usize = 0;
        let raw = unsafe {
            // SAFETY: performs a kernel ecall with no arguments; return value is decoded below.
            ecall0(SYSCALL_YIELD)
        };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Spawns a new task using the provided entry point, stack, and bootstrap endpoint.
#[cfg(nexus_env = "os")]
pub fn spawn(entry_pc: u64, stack_sp: u64, asid: u64, bootstrap_ep: u32) -> SysResult<Pid> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_SPAWN: usize = 7;
        let raw = unsafe {
            // SAFETY: the syscall interface expects raw register arguments and returns the new PID
            // or a sentinel error code; all inputs are forwarded as provided by the caller.
            ecall4(
                SYSCALL_SPAWN,
                entry_pc as usize,
                stack_sp as usize,
                asid as usize,
                bootstrap_ep as usize,
            )
        };
        decode_syscall(raw).map(|pid| pid as Pid)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Transfers a capability from the current task to `dst_task` with intersected `rights`.
#[cfg(nexus_env = "os")]
pub fn cap_transfer(dst_task: Pid, cap: Cap, rights: Rights) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_CAP_TRANSFER: usize = 8;
        let raw = unsafe {
            // SAFETY: forwards raw arguments expected by the kernel capability transfer ABI.
            ecall3(
                SYSCALL_CAP_TRANSFER,
                dst_task as usize,
                cap as usize,
                rights.bits() as usize,
            )
        };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
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

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn decode_syscall(value: usize) -> SysResult<usize> {
    if let Some(err) = AbiError::from_raw(value) {
        Err(err)
    } else {
        Ok(value)
    }
}

// ——— Architecture-specific ecall helpers (riscv64, OS) ———
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[inline(always)]
unsafe fn ecall0(n: usize) -> usize {
    let mut r7 = n;
    let r0: usize;
    core::arch::asm!(
        "ecall",
        inout("a7") r7,
        lateout("a0") r0,
        options(nostack, preserves_flags)
    );
    r0
}
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
