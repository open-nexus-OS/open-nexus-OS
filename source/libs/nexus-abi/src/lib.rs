// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![cfg_attr(
    not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
    forbid(unsafe_code)
)]
#![deny(clippy::all, missing_docs)]

//! CONTEXT: Shared ABI definitions exposed to userland crates
//! OWNERS: @runtime
//! PUBLIC API: MsgHeader, IpcError; OS-only syscalls: yield_, spawn, exit, wait, cap_transfer, as_*, vmo_*, debug_*
//! DEPENDS_ON: no_std (OS), riscv ecall asm (OS), bitflags
//! INVARIANTS: Header is 16 bytes LE; userspace wrappers map to stable kernel syscall IDs
//! ADR: docs/adr/0016-kernel-libs-architecture.md

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
    /// Blocking IPC operation hit its deadline.
    TimedOut,
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

/// Handle identifying a virtual memory object (VMO).
#[cfg(nexus_env = "os")]
pub type Handle = u32;

/// Opaque handle referencing a user address space managed by the kernel.
#[cfg(nexus_env = "os")]
pub type AsHandle = u64;

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
    /// Caller does not have any children to wait on.
    ChildUnavailable,
    /// Requested process identifier does not belong to the caller.
    NoSuchPid,
    /// Syscall arguments were invalid for the requested operation.
    InvalidArgument,
    /// Operation unsupported on the current build target.
    Unsupported,
}

#[cfg(nexus_env = "os")]
impl AbiError {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    fn from_raw(value: usize) -> Option<Self> {
        if (value as isize) >= 0 {
            return None;
        }
        // Kernel returns negative errno values for syscall failures.
        match -(value as isize) as usize {
            38 => Some(Self::InvalidSyscall),     // ENOSYS
            1 => Some(Self::CapabilityDenied),   // EPERM
            22 => Some(Self::InvalidArgument),   // EINVAL
            10 => Some(Self::ChildUnavailable),  // ECHILD
            3 => Some(Self::NoSuchPid),          // ESRCH
            12 => Some(Self::SpawnFailed),       // ENOMEM (best-effort mapping)
            28 => Some(Self::SpawnFailed),       // ENOSPC (best-effort mapping)
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

/// Returns the current monotonic time in nanoseconds (kernel timer).
#[cfg(nexus_env = "os")]
pub fn nsec() -> SysResult<u64> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_NSEC: usize = 1;
        let raw = unsafe { ecall0(SYSCALL_NSEC) };
        decode_syscall(raw).map(|v| v as u64)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Spawns a new task using the provided entry point, stack, bootstrap endpoint, and GP value.
#[cfg(nexus_env = "os")]
pub fn spawn(
    entry_pc: u64,
    stack_sp: u64,
    asid: u64,
    bootstrap_ep: u32,
    global_pointer: u64,
) -> SysResult<Pid> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_SPAWN: usize = 7;
        let raw = unsafe {
            // SAFETY: the syscall interface expects raw register arguments and returns the new PID
            // or a sentinel error code; all inputs are forwarded as provided by the caller.
            ecall5(
                SYSCALL_SPAWN,
                entry_pc as usize,
                stack_sp as usize,
                asid as usize,
                bootstrap_ep as usize,
                global_pointer as usize,
            )
        };
        decode_syscall(raw).map(|pid| pid as Pid)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Loads and spawns a process from an ELF blob using the kernel exec loader.
#[cfg(nexus_env = "os")]
pub fn exec(elf: &[u8], stack_pages: usize, global_pointer: u64) -> SysResult<Pid> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_EXEC: usize = 13;
        if stack_pages == 0 || elf.is_empty() {
            return Err(AbiError::InvalidArgument);
        }
        let raw = unsafe {
            ecall4(
                SYSCALL_EXEC,
                elf.as_ptr() as usize,
                elf.len(),
                stack_pages,
                global_pointer as usize,
            )
        };
        decode_syscall(raw).map(|pid| pid as Pid)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (elf, stack_pages, global_pointer);
        Err(AbiError::Unsupported)
    }
}

/// Loads and spawns a process from an ELF blob using the kernel exec loader (v2).
///
/// v2 additionally provides a per-service name string that the kernel copies into a read-only
/// mapping in the child address space (RFC-0004 provenance floor).
#[cfg(nexus_env = "os")]
pub fn exec_v2(elf: &[u8], stack_pages: usize, global_pointer: u64, service_name: &str) -> SysResult<Pid> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_EXEC_V2: usize = 17;
        if stack_pages == 0 || elf.is_empty() {
            return Err(AbiError::InvalidArgument);
        }
        // Keep the ABI bounded (kernel enforces too).
        if service_name.len() > 64 {
            return Err(AbiError::InvalidArgument);
        }
        let raw = unsafe {
            ecall6(
                SYSCALL_EXEC_V2,
                elf.as_ptr() as usize,
                elf.len(),
                stack_pages,
                global_pointer as usize,
                service_name.as_ptr() as usize,
                service_name.len(),
            )
        };
        decode_syscall(raw).map(|pid| pid as Pid)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (elf, stack_pages, global_pointer, service_name);
        Err(AbiError::Unsupported)
    }
}

/// Terminates the current task with the provided exit `status`.
#[cfg(nexus_env = "os")]
pub fn exit(status: i32) -> ! {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        const SYSCALL_EXIT: usize = 11;
        let _ = ecall1(SYSCALL_EXIT, status as usize);
        core::hint::spin_loop();
        loop {
            core::hint::spin_loop();
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = status;
        loop {
            core::hint::spin_loop();
        }
    }
}

/// Waits for the child identified by `pid` (or any child when `pid <= 0`).
#[cfg(nexus_env = "os")]
pub fn wait(pid: i32) -> SysResult<(Pid, i32)> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_WAIT: usize = 12;
        let (raw_pid, raw_status) = unsafe { ecall1_pair(SYSCALL_WAIT, pid as usize) };
        let pid = decode_syscall(raw_pid)?;
        Ok((pid as Pid, raw_status as i32))
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = pid;
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

/// Creates a new kernel IPC endpoint and returns a capability slot for it.
///
/// This syscall is currently privileged (bootstrap task only).
#[cfg(nexus_env = "os")]
pub fn ipc_endpoint_create(queue_depth: usize) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_ENDPOINT_CREATE: usize = 19;
        if queue_depth == 0 {
            return Err(AbiError::InvalidArgument);
        }
        let raw = unsafe { ecall1(SYSCALL_IPC_ENDPOINT_CREATE, queue_depth) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = queue_depth;
        Err(AbiError::Unsupported)
    }
}

/// Drops the caller's reference to the capability slot identified by `cap`.
#[cfg(nexus_env = "os")]
pub fn cap_close(cap: Cap) -> SysResult<()> {
    let _ = cap;
    Err(AbiError::Unsupported)
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

/// Drops the caller's reference to the VMO represented by `handle`.
#[cfg(nexus_env = "os")]
pub fn vmo_destroy(handle: Handle) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_VMO_DESTROY: usize = 15;
        let raw = unsafe { ecall1(SYSCALL_VMO_DESTROY, handle as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

// ——— Debug print helpers (OS build) ———

/// Writes a single byte to the kernel UART from userspace for debugging.
#[cfg(nexus_env = "os")]
pub fn debug_putc(byte: u8) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_DEBUG_PUTC: usize = 16;
        let raw = unsafe { ecall1(SYSCALL_DEBUG_PUTC, byte as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = byte;
        Err(AbiError::Unsupported)
    }
}

/// Writes a byte slice to the kernel UART for debugging.
#[cfg(nexus_env = "os")]
pub fn debug_write(bytes: &[u8]) -> SysResult<()> {
    for &b in bytes {
        debug_putc(b)?;
    }
    Ok(())
}

/// Writes a line (with trailing '\n') to the kernel UART for debugging.
#[cfg(nexus_env = "os")]
pub fn debug_println(s: &str) -> SysResult<()> {
    debug_write(s.as_bytes())?;
    debug_putc(b'\n')
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
#[allow(unused_assignments)]
#[inline(always)]
unsafe fn ecall0(n: usize) -> usize {
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
unsafe fn ecall1(n: usize, a0: usize) -> usize {
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
unsafe fn ecall1_pair(n: usize, a0: usize) -> (usize, usize) {
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
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
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
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
unsafe fn ecall5(n: usize, a0: usize, a1: usize, a2: usize, a3: usize, a4: usize) -> usize {
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
unsafe fn ecall6(
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
