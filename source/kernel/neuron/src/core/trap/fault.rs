// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Trap diagnostics split out of the former single-file trap.rs:
//! LAST_TRAP/TRAP_RING recording, cause decoding (is_interrupt/describe_cause/
//! fmt_trap), raw-UART exception printers, user-stack dumper, HPM/rdcycle CSR
//! emulation helpers and the trap_symbols nearest-symbol lookup.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: neuron host tests + QEMU marker gates (just test-os / ci-os-smp)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::*;

// ——— diagnostics ———

#[cfg(any(debug_assertions, feature = "trap_ring"))]
static LAST_TRAP: Mutex<Option<TrapFrame>> = Mutex::new(None);
#[cfg(any(debug_assertions, feature = "trap_ring"))]
#[allow(dead_code)]
static TRAP_DIAG_COUNT: Mutex<usize> = Mutex::new(0);

#[cfg(any(debug_assertions, feature = "trap_ring"))]
const TRAP_RING_LEN: usize = 64;
#[cfg(any(debug_assertions, feature = "trap_ring"))]
static TRAP_RING: Mutex<[Option<TrapFrame>; TRAP_RING_LEN]> = Mutex::new([None; TRAP_RING_LEN]);
#[cfg(any(debug_assertions, feature = "trap_ring"))]
static TRAP_RING_IDX: Mutex<usize> = Mutex::new(0);

#[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
#[inline]
pub(super) fn uart_print_exc(scause: usize, sepc: usize, stval: usize) {
    let mut u = crate::uart::raw_writer();
    let _ = u.write_str("EXC: scause=0x");
    uart_write_hex(&mut u, scause);
    let _ = u.write_str(" sepc=0x");
    uart_write_hex(&mut u, sepc);
    let _ = u.write_str(" stval=0x");
    uart_write_hex(&mut u, stval);
    let _ = u.write_str("\n");
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(super) fn dump_user_stack_for_task(task: &task::Task, spaces: &AddressSpaceManager, sp: usize) {
    const STACK_WORDS: usize = 8;
    if sp == 0 {
        return;
    }
    let handle = match task.address_space() {
        Some(h) => h,
        None => return,
    };
    let space = match spaces.get(handle) {
        Ok(space) => space,
        Err(_) => return,
    };
    let page_table = space.page_table();
    const UART_BASE: usize = 0x1000_0000;
    const UART_TX: usize = 0x0;
    const UART_LSR: usize = 0x5;
    const LSR_TX_IDLE: u8 = 1 << 5;
    unsafe {
        let write_byte = |b: u8| {
            while core::ptr::read_volatile((UART_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0 {
            }
            core::ptr::write_volatile((UART_BASE + UART_TX) as *mut u8, b);
        };
        for index in 0..STACK_WORDS {
            for &b in b"[USER-PF] stack +" {
                write_byte(b);
            }
            let offset = index * core::mem::size_of::<usize>();
            write_byte(b'0');
            write_byte(b'x');
            for shift in (0..4).rev() {
                let nibble = ((offset >> (shift * 4)) & 0xf) as u8;
                let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                write_byte(ch);
            }
            for &b in b" = " {
                write_byte(b);
            }
            let addr = sp.wrapping_add(offset);
            if let Some(pa) = page_table.translate(addr) {
                let value = core::ptr::read_volatile(pa as *const usize);
                write_byte(b'0');
                write_byte(b'x');
                for shift in (0..16).rev() {
                    let nibble = ((value >> (shift * 4)) & 0xf) as u8;
                    let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                    write_byte(ch);
                }
            } else {
                for &b in b"<hole>" {
                    write_byte(b);
                }
            }
            write_byte(b'\n');
        }
    }
}

const INTERRUPT_FLAG: usize = usize::MAX - (usize::MAX >> 1);

// ——— HPM CSR emulation helpers ———

#[inline]
#[allow(dead_code)]
fn is_csr_op(inst: u32) -> bool {
    // SYSTEM opcode (0b1110011), funct3 in {001,010,011} => CSRRW/CSRRS/CSRRC
    (inst & 0x7f) == 0b111_0011 && matches!((inst >> 12) & 0x7, 0b001 | 0b010 | 0b011)
}
#[inline]
pub(super) fn is_rdcycle_or_time(inst: u32) -> bool {
    // rdcycle/rdtime encodings are CSRRS with rs1=x0 and CSR=cycle/time
    if (inst & 0x7f) != 0b111_0011 {
        return false;
    }
    let funct3 = (inst >> 12) & 0x7;
    if funct3 != 0b010 {
        return false;
    }
    let csr = ((inst >> 20) & 0x0fff) as u16;
    csr == 0xC00 /*cycle*/ || csr == 0xC01 /*time*/
}
#[inline]
pub(super) fn is_rdinstret(inst: u32) -> bool {
    if (inst & 0x7f) != 0b111_0011 {
        return false;
    }
    let funct3 = (inst >> 12) & 0x7;
    if funct3 != 0b010 {
        return false;
    }
    let csr = ((inst >> 20) & 0x0fff) as u16;
    csr == 0xC02 /*instret*/
}
#[inline]
#[allow(dead_code)]
fn csr_num(inst: u32) -> u16 {
    ((inst >> 20) & 0x0fff) as u16
}
#[inline]
#[allow(dead_code)]
fn rd_index(inst: u32) -> usize {
    ((inst >> 7) & 0x1f) as usize
}

/// Emulate HPM (mhpmcounter{3..31}, mhpmcounterh{3..31}) reads/writes in S-mode by returning 0
/// and advancing sepc by 4. HPM CSRs are M-mode unless M enables access; on typical firmware they are illegal in S.
#[allow(dead_code)]
fn emulate_hpm_csr(frame: &mut TrapFrame, inst: u32) -> bool {
    if !is_csr_op(inst) {
        return false;
    }
    let csr = csr_num(inst);
    let is_hpm = (0x0B03..=0x0B1F).contains(&csr) || (0x0B83..=0x0B9F).contains(&csr);
    if !is_hpm {
        return false;
    }

    let rd = rd_index(inst);
    if rd != 0 {
        frame.set_x(rd, 0);
    } // read-as-zero
    frame.sepc = frame.sepc.wrapping_add(4);
    true
}

#[cfg(any(debug_assertions, feature = "trap_ring"))]
pub fn record(frame: &TrapFrame) {
    *LAST_TRAP.lock() = Some(*frame);
    let mut idx = TRAP_RING_IDX.lock();
    let mut ring = TRAP_RING.lock();
    ring[*idx % TRAP_RING_LEN] = Some(*frame);
    *idx = (*idx + 1) % TRAP_RING_LEN;
}

#[cfg(not(any(debug_assertions, feature = "trap_ring")))]
pub fn record(_frame: &TrapFrame) {}

#[cfg(any(debug_assertions, feature = "trap_ring"))]
pub fn last_trap() -> Option<TrapFrame> {
    *LAST_TRAP.lock()
}

#[cfg(not(any(debug_assertions, feature = "trap_ring")))]
pub fn last_trap() -> Option<TrapFrame> {
    None
}

#[cfg(any(debug_assertions, feature = "trap_ring"))]
pub fn visit_trap_ring(mut f: impl FnMut(usize, &TrapFrame)) {
    let len = TRAP_RING_LEN;
    let start = *TRAP_RING_IDX.lock();
    let ring = TRAP_RING.lock();
    for offset in 0..len {
        let slot = (start + offset) % len;
        if let Some(frame) = &ring[slot] {
            f(slot, frame);
        }
    }
}
#[inline]
pub fn is_interrupt(scause: usize) -> bool {
    scause & INTERRUPT_FLAG != 0
}

#[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
#[allow(dead_code)]
pub fn describe_cause(scause: usize) -> &'static str {
    let code = scause & (usize::MAX >> 1);
    if is_interrupt(scause) {
        match code {
            1 => "SupervisorSoftInt",
            5 => "SupervisorTimerInt",
            9 => "SupervisorExternalInt",
            _ => "Interrupt",
        }
    } else {
        match code {
            0 => "InstructionAddressMisaligned",
            1 => "InstructionAccessFault",
            2 => "IllegalInstruction",
            3 => "Breakpoint",
            4 => "LoadAddressMisaligned",
            5 => "LoadAccessFault",
            6 => "StoreAMOAddressMisaligned",
            7 => "StoreAMOAccessFault",
            8 => "EnvironmentCallFromUMode",
            9 => "EnvironmentCallFromSMode",
            12 => "InstructionPageFault",
            13 => "LoadPageFault",
            15 => "StoreAMOPageFault",
            _ => "Exception",
        }
    }
}

#[cfg(test)]
mod tests_guard_diag {
    use crate::task::UserGuardInfo;

    fn classify(stval: usize, info: UserGuardInfo) -> Option<&'static str> {
        if stval == info.stack_guard_va {
            Some("STACK")
        } else if info.info_guard_va == Some(stval) {
            Some("BOOTINFO")
        } else {
            None
        }
    }

    #[test]
    fn guard_classifier_recognizes_stack_and_bootinfo() {
        let info = UserGuardInfo { stack_guard_va: 0x2000_1000, info_guard_va: Some(0x2000_3000) };
        assert_eq!(classify(0x2000_1000, info), Some("STACK"));
        assert_eq!(classify(0x2000_3000, info), Some("BOOTINFO"));
        assert_eq!(classify(0x1234_5678, info), None);
    }
}

#[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
#[allow(dead_code)]
pub fn fmt_trap<W: Write>(frame: &TrapFrame, f: &mut W) -> fmt::Result {
    writeln!(f, " sepc=0x{:016x}", frame.sepc)?;
    writeln!(f, " scause=0x{:016x} ({})", frame.scause, describe_cause(frame.scause))?;
    writeln!(f, " stval=0x{:016x}", frame.stval)?;
    writeln!(f, " a0..a7 = {:016x?}", &frame.x[10..=17])
}

#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "trap_symbols"))]
#[allow(dead_code)]
mod trap_symbols {
    include!(concat!(env!("OUT_DIR"), "/trap_symbols.rs"));
}
#[cfg(not(all(target_arch = "riscv64", target_os = "none", feature = "trap_symbols")))]
mod trap_symbols {
    #[allow(dead_code)]
    pub static TRAP_SYMBOLS: &[(usize, &str)] = &[];
}

#[allow(dead_code)]
pub(super) fn nearest_symbol(_addr: usize) -> Option<(&'static str, usize)> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        // Binary search in sorted table if present
        let table = &trap_symbols::TRAP_SYMBOLS;
        if table.is_empty() {
            return None;
        }
        let mut lo = 0usize;
        let mut hi = table.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if table[mid].0 <= _addr {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let (base, name) = table[lo - 1];
        Some((name, base))
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        None
    }
}
