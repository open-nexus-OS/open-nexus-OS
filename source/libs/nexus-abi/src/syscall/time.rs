// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Time syscalls — nsec, timers, waitsets, fences
//! (Mechanical split out of the former lib.rs monolith — ADR-0051 hygiene
//! pass; behavior and syscall IDs unchanged.)

#[cfg(nexus_env = "os")]
use super::*;
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

/// Creates a kernel timer capability bound to `notify_ep_cap`.
///
/// `notify_ep_cap` must reference an endpoint capability in the caller's cap table.
/// `interval_ns` configures periodic mode when non-zero (0 = one-shot).
#[cfg(nexus_env = "os")]
pub fn timer_create(notify_ep_cap: Cap, interval_ns: u64) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TIMER_CREATE: usize = 33;
        let raw =
            unsafe { ecall2(SYSCALL_TIMER_CREATE, notify_ep_cap as usize, interval_ns as usize) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (notify_ep_cap, interval_ns);
        Err(AbiError::Unsupported)
    }
}

/// Arms a timer capability with an absolute monotonic `deadline_ns`.
#[cfg(nexus_env = "os")]
pub fn timer_set(timer_cap: Cap, deadline_ns: u64) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TIMER_SET: usize = 34;
        let raw = unsafe { ecall2(SYSCALL_TIMER_SET, timer_cap as usize, deadline_ns as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (timer_cap, deadline_ns);
        Err(AbiError::Unsupported)
    }
}

/// Disarms a previously armed timer capability.
#[cfg(nexus_env = "os")]
pub fn timer_cancel(timer_cap: Cap) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TIMER_CANCEL: usize = 35;
        let raw = unsafe { ecall1(SYSCALL_TIMER_CANCEL, timer_cap as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = timer_cap;
        Err(AbiError::Unsupported)
    }
}

/// Creates an empty **waitset** capability (RFC-0033). A waitset lets a task block
/// on MULTIPLE endpoints at once (commands + a timer-notify + a fence-notify) and
/// wake on the first ready — the first-class replacement for using a recv timeout
/// as a clock. Add members with [`waitset_add`], block with [`waitset_wait`].
#[cfg(nexus_env = "os")]
pub fn waitset_create() -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_WAITSET_CREATE: usize = 38;
        let raw = unsafe { ecall0(SYSCALL_WAITSET_CREATE) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Adds an endpoint (RECV right required) as a member of `waitset_cap`. Bounded:
/// over the member limit rejects with `ResourceExhausted`; a non-endpoint cap
/// rejects with `InvalidArgument`. A timer- or fence-notify endpoint is added the
/// same way, so one waitset unifies command, timer, and completion waits.
#[cfg(nexus_env = "os")]
pub fn waitset_add(waitset_cap: Cap, endpoint_cap: Cap) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_WAITSET_ADD: usize = 39;
        let raw =
            unsafe { ecall2(SYSCALL_WAITSET_ADD, waitset_cap as usize, endpoint_cap as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (waitset_cap, endpoint_cap);
        Err(AbiError::Unsupported)
    }
}

/// Blocks until any member endpoint of `waitset_cap` has a pending message, then
/// returns that member's **slot index** (the order it was added). The caller then
/// `ipc_recv`s that endpoint. `deadline_ns == 0` blocks indefinitely (pacing comes
/// from a timer member's fixed deadline, not from this call — so re-entry never
/// resets a clock); a non-zero deadline returns `TimedOut` when it elapses.
#[cfg(nexus_env = "os")]
pub fn waitset_wait(waitset_cap: Cap, deadline_ns: u64) -> SysResult<u32> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_WAITSET_WAIT: usize = 40;
        let raw =
            unsafe { ecall2(SYSCALL_WAITSET_WAIT, waitset_cap as usize, deadline_ns as usize) };
        decode_syscall(raw).map(|slot| slot as u32)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (waitset_cap, deadline_ns);
        Err(AbiError::Unsupported)
    }
}

/// Creates a timeline **fence** capability (RFC-0033). A fence holds a monotonic `u64`
/// value: producers advance it with [`fence_signal`], consumers block for a target with
/// [`fence_wait`]. It is the completion/ordering primitive for the DriverKit submit ring
/// (a producer signals a sequence number; consumers wait for it).
#[cfg(nexus_env = "os")]
pub fn fence_create() -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_FENCE_CREATE: usize = 41;
        let raw = unsafe { ecall0(SYSCALL_FENCE_CREATE) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Advances `fence_cap` monotonically to at least `value` (a lower value is a no-op) and
/// wakes every waiter the new value now satisfies.
#[cfg(nexus_env = "os")]
pub fn fence_signal(fence_cap: Cap, value: u64) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_FENCE_SIGNAL: usize = 42;
        let raw = unsafe { ecall2(SYSCALL_FENCE_SIGNAL, fence_cap as usize, value as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (fence_cap, value);
        Err(AbiError::Unsupported)
    }
}

/// Blocks until `fence_cap`'s value reaches `target`. `deadline_ns == 0` blocks
/// indefinitely; a non-zero deadline returns `TimedOut` when it elapses. Unlike a
/// recv-timeout clock, the deadline is a fixed wall-clock cap, so re-entry never resets it.
#[cfg(nexus_env = "os")]
pub fn fence_wait(fence_cap: Cap, target: u64, deadline_ns: u64) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_FENCE_WAIT: usize = 43;
        let raw = unsafe {
            ecall3(SYSCALL_FENCE_WAIT, fence_cap as usize, target as usize, deadline_ns as usize)
        };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (fence_cap, target, deadline_ns);
        Err(AbiError::Unsupported)
    }
}
