// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: which unit an instruction needs (RFC-0119) — the decode behind a thread's first
//! use: an illegal instruction of a thread whose floating-point or vector unit is off gives
//! it that unit's state, when the machine offers the unit. Pure; a child of `fp_rule`.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests` below (`test_reject_first_use_by_another_instruction`,
//!   `test_reject_vector_use_without_the_extension`)
//! INVARIANTS: an instruction that is no unit's gives no state; a unit that is not offered
//!   gives no state; a unit that is on already gives no second one
//! RFC: docs/rfcs/RFC-0119-a-thread-s-floating-point-and-vector-registers-are-its-own-kept-from-first-use.md

use super::{Kind, Unit};

/// The units `inst` (the trapped instruction's bits, a compressed one in the low half) needs
/// on: `[float, vector]`. A vector instruction that computes with floats needs both.
#[must_use]
pub const fn needs(inst: u32) -> [bool; 2] {
    if inst & 3 != 3 {
        // Compressed: `c.fld`, `c.fsd` and their stack-pointer forms (64-bit harts).
        let (quadrant, funct3) = (inst & 3, (inst >> 13) & 7);
        return [matches!(quadrant, 0 | 2) && matches!(funct3, 1 | 5), false];
    }
    let funct3 = (inst >> 12) & 7;
    match inst & 0x7f {
        // Loads and stores: the widths 1–4 are the float unit's, the others the vector unit's.
        0x07 | 0x27 => [matches!(funct3, 1..=4), matches!(funct3, 0 | 5..=7)],
        // Fused multiply-adds and every other float operation.
        0x43 | 0x47 | 0x4b | 0x4f | 0x53 => [true, false],
        // Vector operations and the length setters; the float-operand forms need both.
        0x57 => [matches!(funct3, 1 | 5), true],
        // A control-register instruction naming `fflags`, `frm`, `fcsr`; or `vstart`,
        // `vxsat`, `vxrm`, `vcsr`, `vl`, `vtype`, `vlenb`.
        0x73 if funct3 != 0 && funct3 != 4 => {
            let csr = inst >> 20;
            [matches!(csr, 1..=3), matches!(csr, 0x008..=0x00a | 0x00f | 0xc20..=0xc22)]
        }
        _ => [false, false],
    }
}

/// What a thread's illegal instruction asks for: the unit to give it a state of. `None`: the
/// instruction is no unit's, or every unit it needs is on already (it is illegal for another
/// reason), or the unit it needs is not offered on this machine. `inst == 0` — a hart that
/// does not report the instruction — asks for the first offered unit that is off.
#[must_use]
pub const fn asks_for(inst: u32, status: usize, offered: [bool; 2]) -> Option<Kind> {
    let needed = if inst == 0 { [true, true] } else { needs(inst) };
    let mut i = 0;
    while i < Kind::ALL.len() {
        let kind = Kind::ALL[i];
        if needed[i] && matches!(Unit::of(kind, status), Unit::Off) {
            // A needed unit that is off decides: its state, or — not offered — nothing. A
            // hart that names no instruction is asked for what the machine offers.
            if offered[i] {
                return Some(kind);
            }
            if inst != 0 {
                return None;
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: [bool; 2] = [true, true];
    /// `vfadd.vv` (OPFVV): a vector instruction on floats.
    const VFADD: u32 = 0x0220_91d7;
    /// `fadd.d`.
    const FADD: u32 = 0x02a5_7553;

    fn on(kind: Kind, status: usize) -> usize {
        Unit::Initial.set_in(kind, status)
    }

    #[test]
    fn test_reject_first_use_by_another_instruction() {
        // The float unit's own: fld, fsd, fadd.d, fmadd.d, a read of fcsr, c.fld, c.fsdsp.
        for inst in [0x0005_3007u32, 0x00a5_3027, FADD, 0x6aa5_7543, 0x0030_2573, 0x2000, 0xa002] {
            assert_eq!(asks_for(inst, 0, BOTH), Some(Kind::Float), "{inst:#010x}");
        }
        // The vector unit's own: a load (width 0), an integer operation, vsetvli, a whole
        // register store, a read of vlenb, a write of vcsr.
        for inst in
            [0x0205_0007u32, 0x02b5_0557, 0x0c05_7557, 0x0285_80a7, 0xc220_2573, 0x00f5_1073]
        {
            assert_eq!(asks_for(inst, 0, BOTH), Some(Kind::Vector), "{inst:#010x}");
        }
        // No unit's: an integer add, a read of the cycle counter, a cache-block instruction,
        // c.ld — whatever is offered.
        for inst in [0x00b5_0533u32, 0xc000_2573, 0x0015_200f, 0x6000] {
            assert_eq!(asks_for(inst, 0, BOTH), None, "{inst:#010x}");
        }
        // A unit's instruction with the unit on is illegal for another reason.
        assert_eq!(asks_for(FADD, on(Kind::Float, 0), BOTH), None);
    }

    #[test]
    fn a_vector_instruction_on_floats_asks_for_both_units_one_trap_each() {
        assert_eq!(needs(VFADD), [true, true]);
        assert_eq!(asks_for(VFADD, 0, BOTH), Some(Kind::Float));
        let float_on = on(Kind::Float, 0);
        assert_eq!(asks_for(VFADD, float_on, BOTH), Some(Kind::Vector));
        assert_eq!(asks_for(VFADD, on(Kind::Vector, float_on), BOTH), None);
    }

    #[test]
    fn test_reject_vector_use_without_the_extension() {
        // A machine whose harts do not all carry the vector unit: its instructions stay
        // illegal, whatever else is offered — and a float instruction is unaffected.
        for inst in [0x0205_0007u32, 0x02b5_0557, 0x0c05_7557, 0xc220_2573] {
            assert_eq!(asks_for(inst, 0, [true, false]), None, "{inst:#010x}");
            assert_eq!(asks_for(inst, 0, [false, false]), None, "{inst:#010x}");
        }
        assert_eq!(asks_for(FADD, 0, [true, false]), Some(Kind::Float));
        assert_eq!(asks_for(FADD, 0, [false, true]), None);
        // A vector instruction on floats where only the float unit is offered: the float
        // state is given, then the instruction is refused at the vector unit.
        assert_eq!(asks_for(VFADD, 0, [true, false]), Some(Kind::Float));
        assert_eq!(asks_for(VFADD, on(Kind::Float, 0), [true, false]), None);
    }

    #[test]
    fn a_hart_that_names_no_instruction_is_given_what_is_offered_in_order() {
        assert_eq!(asks_for(0, 0, BOTH), Some(Kind::Float));
        let float_on = on(Kind::Float, 0);
        assert_eq!(asks_for(0, float_on, BOTH), Some(Kind::Vector));
        assert_eq!(asks_for(0, 0, [false, true]), Some(Kind::Vector));
        assert_eq!(asks_for(0, 0, [false, false]), None);
        assert_eq!(asks_for(0, on(Kind::Vector, float_on), BOTH), None);
    }
}
