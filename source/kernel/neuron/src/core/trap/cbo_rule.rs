// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the pure rule of ADR-0079 — which user-mode illegal instruction is a cache-block
//! maintenance instruction, and when a process is granted the right to run it. NOT
//! target-gated (`mod trap` is riscv/none-only), so the decode and the decision run on host;
//! fed by `trap::cache_right` on the target.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests` below (decode + decision matrix)
//! ADR: docs/adr/0079-user-cache-maintenance-is-the-right-of-a-non-coherent-devices-capability.md

/// The cache-block maintenance instructions user mode may be granted: `cbo.inval` (imm 0),
/// `cbo.clean` (1), `cbo.flush` (2) — opcode MISC-MEM, funct3 010, rd 0. `cbo.zero` (imm 4,
/// Zicboz) is never granted.
#[cfg(any(test, target_os = "none"))]
pub(crate) fn is_cbo_maintenance(inst: u32) -> bool {
    const MISC_MEM: u32 = 0x0f;
    const FUNCT3_CBO: u32 = 0b010;
    inst & 0x7f == MISC_MEM
        && (inst >> 12) & 0x7 == FUNCT3_CBO
        && (inst >> 7) & 0x1f == 0
        && matches!(inst >> 20, 0..=2)
}

/// What a user-mode illegal instruction means for cache maintenance.
#[cfg(any(test, target_os = "none"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Not a cache-block maintenance instruction: an ordinary illegal instruction.
    NotCbo,
    /// The process holds the right and the hart's bits are off: grant, then retry.
    Grant,
    /// No right, no Zicbom on the harts, or the bits were already on (a retry would trap
    /// again): the process's fault.
    Refuse,
}

/// The decision: `supported` = the harts list Zicbom, `holds_right` = a non-coherent device's
/// capability is in the process's table, `hart_on` = this hart already runs user mode with the
/// bits on.
#[cfg(any(test, target_os = "none"))]
pub(crate) fn decide(inst: u32, supported: bool, holds_right: bool, hart_on: bool) -> Verdict {
    if !is_cbo_maintenance(inst) {
        Verdict::NotCbo
    } else if supported && holds_right && !hart_on {
        Verdict::Grant
    } else {
        Verdict::Refuse
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CBO_INVAL_SP: u32 = 0x0001_200f;
    const CBO_CLEAN_SP: u32 = 0x0011_200f;
    const CBO_FLUSH_A0: u32 = 0x0025_200f;
    const CBO_ZERO_SP: u32 = 0x0041_200f;

    #[test]
    fn decodes_the_three_maintenance_instructions() {
        assert!(is_cbo_maintenance(CBO_INVAL_SP));
        assert!(is_cbo_maintenance(CBO_CLEAN_SP));
        assert!(is_cbo_maintenance(CBO_FLUSH_A0));
    }

    #[test]
    fn test_reject_cbo_zero_and_other_misc_mem() {
        assert!(!is_cbo_maintenance(CBO_ZERO_SP)); // Zicboz is never granted
        assert!(!is_cbo_maintenance(0x0ff0_000f)); // fence
        assert!(!is_cbo_maintenance(0x0000_100f)); // fence.i
        assert!(!is_cbo_maintenance(CBO_CLEAN_SP | (1 << 7))); // rd != 0
        assert!(!is_cbo_maintenance(0));
    }

    #[test]
    fn grants_a_holder_once_per_hart() {
        assert_eq!(decide(CBO_CLEAN_SP, true, true, false), Verdict::Grant);
    }

    #[test]
    fn test_reject_user_cache_flush_without_right() {
        assert_eq!(decide(CBO_FLUSH_A0, true, false, false), Verdict::Refuse);
        assert_eq!(decide(CBO_INVAL_SP, true, false, true), Verdict::Refuse);
    }

    #[test]
    fn test_reject_cbo_without_zicbom_or_on_a_hart_already_on() {
        // No Zicbom on the harts: nothing to grant.
        assert_eq!(decide(CBO_CLEAN_SP, false, true, false), Verdict::Refuse);
        // The bits were on and it still trapped: a retry would trap forever.
        assert_eq!(decide(CBO_CLEAN_SP, true, true, true), Verdict::Refuse);
    }

    #[test]
    fn an_ordinary_illegal_instruction_is_not_cbo() {
        assert_eq!(decide(0xffff_ffff, true, true, false), Verdict::NotCbo);
    }
}
