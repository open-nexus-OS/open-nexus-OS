// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Capability + endpoint + IRQ syscalls — cap transfer/clone/close, endpoint create/close, irq bind/complete
//! (Mechanical split out of the former lib.rs monolith — ADR-0051 hygiene
//! pass; behavior and syscall IDs unchanged.)

#[cfg(nexus_env = "os")]
use super::*;
/// Binds an external interrupt source (PLIC) to an endpoint the caller owns, so
/// the kernel routes that device IRQ to `endpoint_cap` and wakes a blocked
/// receiver — the reactive alternative to polling the device. The driver then
/// blocks on `recv(endpoint)` and calls [`irq_complete`] after servicing it.
#[cfg(nexus_env = "os")]
pub fn irq_bind(irq: u32, endpoint_cap: Cap) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IRQ_BIND: usize = 36;
        let raw = unsafe { ecall2(SYSCALL_IRQ_BIND, irq as usize, endpoint_cap as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (irq, endpoint_cap);
        Err(AbiError::Unsupported)
    }
}

/// Acknowledges a delivered IRQ so the PLIC can re-arm it. Call after the device
/// has been serviced (its interrupt condition cleared, e.g. virtqueue drained).
#[cfg(nexus_env = "os")]
pub fn irq_complete(irq: u32) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IRQ_COMPLETE: usize = 37;
        let raw = unsafe { ecall1(SYSCALL_IRQ_COMPLETE, irq as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = irq;
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
            ecall3(SYSCALL_CAP_TRANSFER, dst_task as usize, cap as usize, rights.bits() as usize)
        };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Transfers a capability from the current task to `dst_task` into `dst_slot`.
#[cfg(nexus_env = "os")]
pub fn cap_transfer_to_slot(
    dst_task: Pid,
    cap: Cap,
    rights: Rights,
    dst_slot: Cap,
) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_CAP_TRANSFER_TO: usize = 31;
        let raw = unsafe {
            ecall4(
                SYSCALL_CAP_TRANSFER_TO,
                dst_task as usize,
                cap as usize,
                rights.bits() as usize,
                dst_slot as usize,
            )
        };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (dst_task, cap, rights, dst_slot);
        Err(AbiError::Unsupported)
    }
}

/// Creates a new kernel IPC endpoint and returns a capability slot for it.
///
/// Bring-up rule: this syscall is currently restricted to init-lite (the direct child of the
/// bootstrap task, parent PID 0), acting as the temporary endpoint factory (RFC-0005 Phase 2
/// hardening).
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

/// Creates a new kernel IPC endpoint using an endpoint-factory capability slot.
///
/// This is the hardened replacement for `ipc_endpoint_create()` (v1). The caller must hold a
/// `CapabilityKind::EndpointFactory` capability with `Rights::MANAGE` in `factory_cap`.
#[cfg(nexus_env = "os")]
pub fn ipc_endpoint_create_v2(factory_cap: Cap, queue_depth: usize) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_ENDPOINT_CREATE_V2: usize = 22;
        if queue_depth == 0 {
            return Err(AbiError::InvalidArgument);
        }
        let raw =
            unsafe { ecall3(SYSCALL_IPC_ENDPOINT_CREATE_V2, factory_cap as usize, queue_depth, 0) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (factory_cap, queue_depth);
        Err(AbiError::Unsupported)
    }
}

/// Creates a new kernel IPC endpoint and assigns ownership to `owner_pid`.
///
/// This is a bootstrap helper used by init-lite so endpoints created during bring-up can be owned
/// by the target service (close-on-exit semantics), while init-lite retains the creator capability
/// for rights-filtered distribution.
#[cfg(nexus_env = "os")]
pub fn ipc_endpoint_create_for(
    factory_cap: Cap,
    owner_pid: u32,
    queue_depth: usize,
) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_ENDPOINT_CREATE_FOR: usize = 23;
        if queue_depth == 0 {
            return Err(AbiError::InvalidArgument);
        }
        let raw = unsafe {
            ecall3(
                SYSCALL_IPC_ENDPOINT_CREATE_FOR,
                factory_cap as usize,
                owner_pid as usize,
                queue_depth,
            )
        };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (factory_cap, owner_pid, queue_depth);
        Err(AbiError::Unsupported)
    }
}

/// Closes an IPC endpoint referenced by `cap` (slot id) if the capability includes `Rights::MANAGE`.
///
/// This is a *global close* (revocation-by-close): once closed, subsequent IPC operations on the
/// endpoint fail deterministically (`NoSuchEndpoint`).
#[cfg(nexus_env = "os")]
pub fn ipc_endpoint_close(cap: Cap) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_ENDPOINT_CLOSE: usize = 21;
        let raw = unsafe { ecall1(SYSCALL_IPC_ENDPOINT_CLOSE, cap as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = cap;
        Err(AbiError::Unsupported)
    }
}

/// Drops the caller's reference to the capability slot identified by `cap`.
#[cfg(nexus_env = "os")]
pub fn cap_close(cap: Cap) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_CAP_CLOSE: usize = 20;
        let raw = unsafe { ecall1(SYSCALL_CAP_CLOSE, cap as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = cap;
        Err(AbiError::Unsupported)
    }
}

/// Clones a capability slot locally.
///
/// Returns the newly allocated slot in the caller. This is a local duplicate only (no transfer).
#[cfg(nexus_env = "os")]
pub fn cap_clone(cap: Cap) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_CAP_CLONE: usize = 24;
        let raw = unsafe { ecall1(SYSCALL_CAP_CLONE, cap as usize) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = cap;
        Err(AbiError::Unsupported)
    }
}
