// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: IPC v1/v2 syscalls — send/recv wrappers + IpcRecvV2Desc (kernel/userspace syscall ABI)
//! (Mechanical split out of the former lib.rs monolith — ADR-0051 hygiene
//! pass; behavior and syscall IDs unchanged.)

#[cfg(nexus_env = "os")]
use super::*;
// ——— IPC v1 syscalls (OS build) ———

/// Syscall flags for IPC v1 operations.
#[cfg(nexus_env = "os")]
pub const IPC_SYS_NONBLOCK: u32 = 1 << 0;
/// Permit payload truncation on receive.
#[cfg(nexus_env = "os")]
pub const IPC_SYS_TRUNCATE: u32 = 1 << 1;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn decode_ipc_send(value: usize) -> Result<usize> {
    if (value as isize) < 0 {
        match -(value as isize) as usize {
            1 => Err(IpcError::PermissionDenied), // EPERM
            3 => Err(IpcError::NoSuchEndpoint),   // ESRCH
            11 => Err(IpcError::QueueFull),       // EAGAIN
            28 => Err(IpcError::NoSpace),         // ENOSPC
            110 => Err(IpcError::TimedOut),       // ETIMEDOUT
            _ => Err(IpcError::Unsupported),
        }
    } else {
        Ok(value)
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn decode_ipc_recv(value: usize) -> Result<usize> {
    if (value as isize) < 0 {
        match -(value as isize) as usize {
            1 => Err(IpcError::PermissionDenied), // EPERM
            3 => Err(IpcError::NoSuchEndpoint),   // ESRCH
            11 => Err(IpcError::QueueEmpty),      // EAGAIN
            28 => Err(IpcError::NoSpace),         // ENOSPC
            110 => Err(IpcError::TimedOut),       // ETIMEDOUT
            _ => Err(IpcError::Unsupported),
        }
    } else {
        Ok(value)
    }
}

/// Sends an IPC v1 message to the endpoint referenced by `slot` (payload copy-in).
///
/// `sys_flags` uses [`IPC_SYS_NONBLOCK`]. When `sys_flags` does not include NONBLOCK, the
/// kernel may block until the queue has capacity or the optional `deadline_ns` expires.
///
/// `deadline_ns=0` means “no deadline”.
#[cfg(nexus_env = "os")]
pub fn ipc_send_v1(
    slot: Cap,
    header: &MsgHeader,
    payload: &[u8],
    sys_flags: u32,
    deadline_ns: u64,
) -> Result<usize> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_SEND_V1: usize = 14;
        let header_ptr = header as *const MsgHeader as usize;
        let payload_ptr = payload.as_ptr() as usize;
        let payload_len = payload.len();
        let sys_flags = sys_flags as usize;
        let deadline_ns = deadline_ns as usize;
        let raw = unsafe {
            ecall6(
                SYSCALL_IPC_SEND_V1,
                slot as usize,
                header_ptr,
                payload_ptr,
                payload_len,
                sys_flags,
                deadline_ns,
            )
        };
        decode_ipc_send(raw)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (slot, header, payload, sys_flags, deadline_ns);
        Err(IpcError::Unsupported)
    }
}

/// Convenience helper: non-blocking send with no deadline.
#[cfg(nexus_env = "os")]
pub fn ipc_send_v1_nb(slot: Cap, header: &MsgHeader, payload: &[u8]) -> Result<usize> {
    ipc_send_v1(slot, header, payload, IPC_SYS_NONBLOCK, 0)
}

/// Receives an IPC v1 message from the endpoint referenced by `slot` (payload copy-out).
///
/// Returns the number of bytes written into `payload_out`.
///
/// `sys_flags` uses [`IPC_SYS_NONBLOCK`] and [`IPC_SYS_TRUNCATE`]. When `sys_flags` does not
/// include NONBLOCK, the kernel may block until a message arrives or the optional
/// `deadline_ns` expires.
///
/// `deadline_ns=0` means “no deadline”.
#[cfg(nexus_env = "os")]
pub fn ipc_recv_v1(
    slot: Cap,
    header_out: &mut MsgHeader,
    payload_out: &mut [u8],
    sys_flags: u32,
    deadline_ns: u64,
) -> Result<usize> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_RECV_V1: usize = 18;
        let header_out_ptr = header_out as *mut MsgHeader as usize;
        let payload_out_ptr = payload_out.as_mut_ptr() as usize;
        let payload_out_max = payload_out.len();
        let sys_flags = sys_flags as usize;
        let deadline_ns = deadline_ns as usize;
        let raw = unsafe {
            ecall6(
                SYSCALL_IPC_RECV_V1,
                slot as usize,
                header_out_ptr,
                payload_out_ptr,
                payload_out_max,
                sys_flags,
                deadline_ns,
            )
        };
        decode_ipc_recv(raw)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (slot, header_out, payload_out, sys_flags, deadline_ns);
        Err(IpcError::Unsupported)
    }
}

/// IPC recv v2 descriptor (extensible ABI for recv-side metadata).
///
/// This struct is part of the **kernel/userspace syscall ABI** and is therefore treated as
/// layout-stable. Host builds keep the definition available so we can run layout tests without
/// needing an OS test runner.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IpcRecvV2Desc {
    /// Descriptor magic ('N''X''I''2').
    pub magic: u32,
    /// Descriptor version.
    pub version: u32,
    /// Receive endpoint capability slot.
    pub slot: u32,
    /// Reserved padding.
    pub _pad0: u32,
    /// User pointer to `MsgHeader` to be written by the kernel.
    pub header_out_ptr: u64,
    /// User pointer to payload buffer to be written by the kernel.
    pub payload_out_ptr: u64,
    /// Maximum payload bytes the kernel may write.
    pub payload_out_max: u64,
    /// User pointer to `u64` where the kernel writes `sender_service_id`.
    pub sender_service_id_out_ptr: u64,
    /// Syscall flags (NONBLOCK/TRUNCATE).
    pub sys_flags: u32,
    /// Reserved padding.
    pub _pad1: u32,
    /// Deadline in nanoseconds (`0` means no deadline).
    pub deadline_ns: u64,
}

/// `IpcRecvV2Desc` magic (`'N''X''I''2'`).
pub const IPC_RECV_V2_DESC_MAGIC: u32 = u32::from_be_bytes(*b"NXI2");
/// `IpcRecvV2Desc` version.
pub const IPC_RECV_V2_DESC_VERSION: u32 = 1;

/// Receives an IPC message and additionally returns the sender's kernel-derived service identity.
///
/// This is a descriptor-based syscall (v2) so we can extend metadata without being limited by
/// the register argument count.
#[cfg(nexus_env = "os")]
pub fn ipc_recv_v2(
    slot: Cap,
    header_out: &mut MsgHeader,
    payload_out: &mut [u8],
    sender_service_id_out: &mut u64,
    sys_flags: u32,
    deadline_ns: u64,
) -> Result<usize> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_RECV_V2: usize = 26;
        let desc = IpcRecvV2Desc {
            magic: IPC_RECV_V2_DESC_MAGIC,
            version: IPC_RECV_V2_DESC_VERSION,
            slot: slot as u32,
            _pad0: 0,
            header_out_ptr: header_out as *mut MsgHeader as u64,
            payload_out_ptr: payload_out.as_mut_ptr() as u64,
            payload_out_max: payload_out.len() as u64,
            sender_service_id_out_ptr: sender_service_id_out as *mut u64 as u64,
            sys_flags,
            _pad1: 0,
            deadline_ns,
        };
        let raw = unsafe { ecall1(SYSCALL_IPC_RECV_V2, &desc as *const _ as usize) };
        decode_ipc_recv(raw)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (slot, header_out, payload_out, sender_service_id_out, sys_flags, deadline_ns);
        Err(IpcError::Unsupported)
    }
}

/// Convenience helper: non-blocking receive (optionally truncating) with no deadline.
#[cfg(nexus_env = "os")]
pub fn ipc_recv_v1_nb(
    slot: Cap,
    header_out: &mut MsgHeader,
    payload_out: &mut [u8],
    truncate: bool,
) -> Result<usize> {
    let mut flags = IPC_SYS_NONBLOCK;
    if truncate {
        flags |= IPC_SYS_TRUNCATE;
    }
    ipc_recv_v1(slot, header_out, payload_out, flags, 0)
}
