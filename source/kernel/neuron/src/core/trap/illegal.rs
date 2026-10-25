// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the illegal-instruction exception, split out of `handler.rs` (module-size ratchet).
//! Whitelisted counter reads are emulated; from user mode, a cache-maintenance instruction may be
//! granted and retried (ADR-0079, `cache_right`) and anything else kills the PROCESS with
//! reason=fault (cause IllegalInstruction) — before TASK-0333 6c-1a a user illegal instruction
//! panicked the kernel. From S-mode it is a kernel bug: diagnostics, then the panic.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `cbo_rule` host tests; QEMU `SELFTEST: cache maintenance refused ok`
//! ADR: docs/adr/0079-user-cache-maintenance-is-the-right-of-a-non-coherent-devices-capability.md

use super::*;

/// Handle an illegal-instruction trap; returns to the trap epilogue (emulated, granted, or the
/// user process killed) or panics for an S-mode one.
pub(super) fn handle(frame: &mut TrapFrame) {
    // Decode from stval (avoid touching faulting PC); emulate only whitelisted CSR reads.
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    let inst = riscv::register::stval::read() as u32;
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    let inst: u32 = 0;
    if super::fault::is_rdcycle_or_time(inst) || super::fault::is_rdinstret(inst) {
        let rd = ((inst >> 7) & 0x1f) as usize;
        if rd != 0 {
            frame.set_x(rd, 0);
        }
        super::fault::record(frame);
        frame.sepc = frame.sepc.wrapping_add(4);
        return;
    }
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SSTATUS_SPP: usize = 1 << 8;
        if frame.sstatus & SSTATUS_SPP == 0 {
            if super::cache_right::try_grant(frame, inst as usize) {
                return;
            }
            report_user(frame, inst);
            super::fault::record(frame);
            super::fault::kill_faulting_user_task(frame);
            return;
        }
    }
    report_kernel(frame);
    super::fault::record(frame);
    // Avoid formatted panic to prevent allocator/formatting faults during bring-up
    panic!("ILLEGAL");
}

/// One head line for a user process killed by an illegal instruction.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn report_user(frame: &TrapFrame, inst: u32) {
    use core::fmt::Write as _;
    let pid = super::runtime::runtime_kernel_handles_diagnostic()
        // SAFETY: the diagnostic handles point at the live task table (as the user page-fault
        // line reads them); a read of the current pid only.
        .map(|handles| unsafe { handles.tasks.as_ref() }.current_pid().as_index())
        .unwrap_or(usize::MAX);
    let mut u = crate::uart::raw_writer();
    let _ = writeln!(u, "[USER-ILL] sepc=0x{:x} inst=0x{:08x} pid=0x{:04x}", frame.sepc, inst, pid);
}

/// Precise diagnostics for an S-mode illegal instruction: sepc, ra, stval, the instruction bytes
/// at sepc and the PTE of its page.
fn report_kernel(frame: &TrapFrame) {
    use core::fmt::Write as _;
    let mut u = crate::uart::raw_writer();
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    let stval_now = riscv::register::stval::read();
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    let stval_now: usize = 0;
    let _ = writeln!(
        u,
        "ILLEGAL-D: sepc=0x{:x} ra=0x{:x} stval=0x{:x}",
        frame.sepc, frame.x[1], stval_now
    );
    // Best-effort fetch of instruction bytes at sepc
    let i16 = unsafe { core::ptr::read_volatile(frame.sepc as *const u16) } as u16;
    let i32 = unsafe { core::ptr::read_volatile(frame.sepc as *const u32) } as u32;
    let _ = writeln!(u, "ILLEGAL-D: inst16=0x{:04x} inst32=0x{:08x}", i16, i32);
    // Extra: dump PTE flags for sepc page if available (debug aid)
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        // Walk the active SATP to locate the PTE for the faulting page and dump flags.
        fn vpn_indices_sv39(va: usize) -> [usize; 3] {
            let vpn0 = (va >> 12) & 0x1ff;
            let vpn1 = (va >> 21) & 0x1ff;
            let vpn2 = (va >> 30) & 0x1ff;
            [vpn2, vpn1, vpn0] // hardware order: L2->L1->L0
        }
        let satp_now = riscv::register::satp::read().bits();
        let ppn = satp_now & ((1 << 44) - 1);
        if ppn == 0 {
            let page_va = frame.sepc & !(crate::mm::PAGE_SIZE - 1);
            let _ = writeln!(u, "ILLEGAL-D: satp=0x{:x} page=0x{:x} (ppn=0)", satp_now, page_va);
        } else {
            let mut table = crate::phys::phys_to_virt(ppn << 12) as *const usize;
            let indices = vpn_indices_sv39(frame.sepc);
            let mut pte: usize = 0;
            let mut found = true;
            for (level, idx) in indices.iter().enumerate() {
                let entry_ptr = unsafe { table.add(*idx) };
                let entry = unsafe { core::ptr::read_volatile(entry_ptr) };
                if entry & 1 == 0 {
                    found = false;
                    break;
                }
                let is_leaf = (entry & ((1 << 1) | (1 << 2) | (1 << 3))) != 0; // any of R/W/X
                if level == 2 {
                    if !is_leaf {
                        found = false;
                        break;
                    }
                    pte = entry;
                    break;
                }
                if is_leaf {
                    found = false;
                    break;
                }
                let next_ppn = (entry >> 10) & ((1 << 44) - 1);
                table = crate::phys::phys_to_virt(next_ppn << 12) as *const usize;
            }
            if found {
                let flags = pte & 0x3ff;
                let _ = writeln!(
                    u,
                    "ILLEGAL-D: satp=0x{:x} pte=0x{:x} flags=0x{:x}",
                    satp_now, pte, flags
                );
            } else {
                let page_va = frame.sepc & !(crate::mm::PAGE_SIZE - 1);
                let _ = writeln!(
                    u,
                    "ILLEGAL-D: satp=0x{:x} page=0x{:x} (unmapped or non-leaf)",
                    satp_now, page_va
                );
            }
        }
    }
}
