// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Task lifecycle syscalls — yield/pid/qos/sched/spawn/thread/exec/exit/wait
//! (Mechanical split out of the former lib.rs monolith — ADR-0051 hygiene
//! pass; behavior and syscall IDs unchanged.)

#[cfg(nexus_env = "os")]
use super::*;
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

/// Returns the current task PID.
#[cfg(nexus_env = "os")]
pub fn pid() -> SysResult<u32> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_GETPID: usize = 25;
        let raw = unsafe { ecall0(SYSCALL_GETPID) };
        decode_syscall(raw).map(|v| v as u32)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Returns the current task's scheduler QoS hint.
#[cfg(nexus_env = "os")]
#[must_use = "qos get result must be handled"]
pub fn task_qos_get() -> SysResult<QosClass> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TASK_QOS: usize = 15;
        const TASK_QOS_OP_GET_SELF: usize = 0;
        let raw = unsafe { ecall3(SYSCALL_TASK_QOS, TASK_QOS_OP_GET_SELF, 0, 0) };
        decode_syscall(raw)
            .and_then(|value| QosClass::from_u8(value as u8).ok_or(AbiError::InvalidArgument))
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Sets the current task's scheduler QoS hint.
///
/// Policy:
/// - equal/lower transitions are allowed for self;
/// - upward transitions require the privileged set-for-target path.
#[cfg(nexus_env = "os")]
#[must_use = "qos set-self result must be handled"]
pub fn task_qos_set_self(qos: QosClass) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TASK_QOS: usize = 15;
        const TASK_QOS_OP_SET: usize = 1;
        let target = pid()? as usize;
        let raw = unsafe { ecall3(SYSCALL_TASK_QOS, TASK_QOS_OP_SET, target, qos as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = qos;
        Err(AbiError::Unsupported)
    }
}

/// B (TASK-0042): scheduling-attribute ops via SYSCALL_SCHED=46.
/// target 0 = self; cross-task requires the QoS-admin capability.
#[cfg(nexus_env = "os")]
pub mod sched {
    use super::*;

    const SYSCALL_SCHED: usize = 48;
    const OP_GET_AFFINITY: usize = 0;
    const OP_SET_AFFINITY: usize = 1;
    const OP_GET_SHARES: usize = 2;
    const OP_SET_SHARES: usize = 3;

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    fn op(op: usize, target: usize, value: usize) -> SysResult<usize> {
        // SAFETY: plain syscall; the kernel validates every argument.
        let raw = unsafe { ecall3(SYSCALL_SCHED, op, target, value) };
        decode_syscall(raw)
    }

    /// Returns the caller's CPU affinity mask (bit N = may run on CPU N).
    #[must_use = "affinity get result must be handled"]
    pub fn get_affinity() -> SysResult<usize> {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            op(OP_GET_AFFINITY, 0, 0)
        }
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        {
            Err(AbiError::Unsupported)
        }
    }

    /// Sets the caller's CPU affinity mask. The kernel validates: non-empty,
    /// within the CPU ceiling, intersecting the online set.
    #[must_use = "affinity set result must be handled"]
    pub fn set_affinity(mask: usize) -> SysResult<()> {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            op(OP_SET_AFFINITY, 0, mask).map(|_| ())
        }
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        {
            let _ = mask;
            Err(AbiError::Unsupported)
        }
    }

    /// Returns the caller's scheduling shares [1, 1000].
    #[must_use = "shares get result must be handled"]
    pub fn get_shares() -> SysResult<usize> {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            op(OP_GET_SHARES, 0, 0)
        }
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        {
            Err(AbiError::Unsupported)
        }
    }

    /// Sets the caller's scheduling shares; the kernel clamps to [1, 1000]
    /// and returns the applied value.
    #[must_use = "shares set result must be handled"]
    pub fn set_shares(shares: usize) -> SysResult<usize> {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            op(OP_SET_SHARES, 0, shares)
        }
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        {
            let _ = shares;
            Err(AbiError::Unsupported)
        }
    }

    /// Emit the kernel's BKL budget gate line (P0; late in the boot ladder
    /// so the report covers the bring-up contention window).
    pub fn bkl_budget_report() {
        let _ = op(4, 0, 0);
    }

    /// Log the bring-up burst maxima and reset the accounting (P0 two-window:
    /// called once bring-up completes; the gate then judges steady state).
    pub fn bkl_budget_reset() {
        let _ = op(5, 0, 0);
    }

    /// Cross-task affinity (B4: execd applies declarative sched recipes).
    /// Requires QoS-admin standing in the kernel (execd/policyd).
    #[must_use = "sched outcomes must be handled"]
    pub fn set_affinity_for(pid: u32, mask: usize) -> SysResult<()> {
        op(OP_SET_AFFINITY, pid as usize, mask).map(|_| ())
    }

    /// Cross-task shares (B4).
    #[must_use = "sched outcomes must be handled"]
    pub fn set_shares_for(pid: u32, shares: usize) -> SysResult<()> {
        op(OP_SET_SHARES, pid as usize, shares).map(|_| ())
    }
}

/// Sets another task's scheduler QoS hint (privileged path).
#[cfg(nexus_env = "os")]
#[must_use = "qos set-for-target result must be handled"]
pub fn task_qos_set_for(target: Pid, qos: QosClass) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TASK_QOS: usize = 15;
        const TASK_QOS_OP_SET: usize = 1;
        let raw =
            unsafe { ecall3(SYSCALL_TASK_QOS, TASK_QOS_OP_SET, target as usize, qos as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (target, qos);
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

/// C (Phase C): same-address-space threads for COMPUTE work.
///
/// Contract (v1, TASK-0276 policy):
/// - A thread shares the parent's address space but has an EMPTY capability
///   table — it cannot do capability IPC. Threads are for computation; the
///   owning task keeps all service communication single-threaded.
/// - The caller provides the stack (no guard page in v1 — document your
///   sizes; the kernel-side spawn validates alignment only).
/// - No TLS in v1: `tp` is free for a user-defined context pointer.
/// - On return from `entry`, the thread exits with status 0 (trampoline).
///   The parent reaps it via `wait(pid)`.
#[cfg(nexus_env = "os")]
pub mod thread {
    use super::*;

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    core::arch::global_asm!(
        r#"
        .section .text.__nexus_thread_trampoline, "ax", @progbits
        .globl __nexus_thread_trampoline
        .align 2
    __nexus_thread_trampoline:
        /* stack top layout (set up by spawn_thread): [entry, arg] */
        ld    t0, 0(sp)
        ld    a0, 8(sp)
        addi  sp, sp, 16
        jalr  ra, t0, 0
        /* entry returned: exit(0) */
        li    a7, 11
        li    a0, 0
        ecall
    1:  j 1b
    "#
    );

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    extern "C" {
        fn __nexus_thread_trampoline();
    }

    /// Spawns a compute thread into the caller's address space, running
    /// `entry(arg)` on the provided stack. Returns the thread's task id;
    /// reap it with [`super::wait`] after it exits.
    #[must_use = "thread spawn result must be handled"]
    pub fn spawn_thread(
        entry: extern "C" fn(usize),
        arg: usize,
        stack: &mut [u8],
    ) -> SysResult<Pid> {
        let pid = spawn_thread_suspended(entry, arg, stack)?;
        task_resume(pid)?;
        Ok(pid)
    }

    /// Like [`spawn_thread`] but leaves the thread SUSPENDED so the parent
    /// can transfer capabilities (e.g. fence caps for a workpool) into the
    /// thread's empty cap table before releasing it via [`super::task_resume`].
    #[must_use = "thread spawn result must be handled"]
    pub fn spawn_thread_suspended(
        entry: extern "C" fn(usize),
        arg: usize,
        stack: &mut [u8],
    ) -> SysResult<Pid> {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            if stack.len() < 1024 {
                return Err(AbiError::InvalidArgument);
            }
            let top = (stack.as_ptr() as usize + stack.len()) & !15usize;
            let sp = top - 16;
            // SAFETY: sp/sp+8 lie inside the caller-provided stack slice.
            unsafe {
                core::ptr::write(sp as *mut usize, entry as usize);
                core::ptr::write((sp + 8) as *mut usize, arg);
            }
            let gp: usize;
            // SAFETY: reading gp is side-effect free; the thread shares our
            // address space and must use the same global pointer.
            unsafe {
                core::arch::asm!("mv {g}, gp", g = out(reg) gp, options(nomem, nostack, preserves_flags));
            }
            let handle = as_self()?;
            // Same-AS spawns start suspended by kernel contract.
            spawn(__nexus_thread_trampoline as usize as u64, sp as u64, handle as u64, 0, gp as u64)
        }
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        {
            let _ = (entry, arg, stack);
            Err(AbiError::Unsupported)
        }
    }
}

/// Resumes a suspended task (enqueues into scheduler). Only callable by the parent.
/// Returns `Ok(())` on success, `Err(InvalidArgument)` if the task is not suspended.
#[cfg(nexus_env = "os")]
pub fn task_resume(pid: Pid) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TASK_RESUME: usize = 32;
        let raw = unsafe { ecall1(SYSCALL_TASK_RESUME, pid as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = pid;
        Err(AbiError::Unsupported)
    }
}

/// Returns the last spawn failure reason for the current task (RFC-0013).
#[cfg(nexus_env = "os")]
pub fn spawn_last_error() -> SysResult<SpawnFailReason> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_SPAWN_LAST_ERROR: usize = 29;
        let raw = unsafe { ecall0(SYSCALL_SPAWN_LAST_ERROR) };
        decode_syscall(raw).map(|v| SpawnFailReason::from_u8(v as u8))
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
pub fn exec_v2(
    elf: &[u8],
    stack_pages: usize,
    global_pointer: u64,
    service_name: &str,
) -> SysResult<Pid> {
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
