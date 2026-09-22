// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel as a position-independent image (RFC-0098 C6, TASK-0245
//! P2). The link base is 0; the boot wrapper's `_start` applies the
//! `R_RISCV_RELATIVE` table for the address the previous stage loaded the image
//! at (nxboot: the lowest free 2 MiB-aligned window of the first memory bank;
//! a direct `-kernel` boot: the firmware's supervisor entry). This module knows
//! where the image ended up — from the linker symbols, which are PC-relative
//! like every other address the kernel computes — and proves the fixup: the
//! table is re-walked and any entry type the entry code does not handle is
//! named in the FAIL marker.
//!
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder — every profile requires
//!   `KSELFTEST: kernel image ok (`; the same kernel boots at two addresses
//!   (nxboot's window and the direct-kernel entry) in the ladder
//! RFC: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md

extern "C" {
    static __image_start: u8;
    static __image_end: u8;
    static __rela_dyn_start: u8;
    static __rela_dyn_end: u8;
}

/// The one relocation type a static PIE of PC-relative code needs.
const R_RISCV_RELATIVE: u64 = 3;
/// One `Elf64_Rela` entry.
const RELA_ENTRY: usize = 24;

/// The image's physical range `[start, end)` as loaded: text through the
/// kernel stack.
pub fn range() -> (usize, usize) {
    // SAFETY: linker-defined symbols; only their addresses are taken.
    unsafe { (&__image_start as *const u8 as usize, &__image_end as *const u8 as usize) }
}

/// The load address (= the fixup delta the entry code applied).
pub fn base() -> usize {
    range().0
}

/// Walk the fixup table: `(entries, first unsupported type)`.
fn relocations() -> (usize, Option<u64>) {
    // SAFETY: linker-defined bounds of the read-only `.rela.dyn` section; the
    // entries are 8-aligned 24-byte records the linker wrote.
    let (start, end) =
        unsafe { (&__rela_dyn_start as *const u8 as usize, &__rela_dyn_end as *const u8 as usize) };
    let mut count = 0;
    let mut bad = None;
    let mut at = start;
    while at + RELA_ENTRY <= end {
        // SAFETY: inside the section bounds checked above; aligned reads.
        let info = unsafe { core::ptr::read_volatile((at + 8) as *const u64) };
        let ty = info & 0xffff_ffff;
        if ty != R_RISCV_RELATIVE && bad.is_none() {
            bad = Some(ty);
        }
        count += 1;
        at += RELA_ENTRY;
    }
    (count, bad)
}

/// Print where the image runs and that its fixup table held only what the
/// entry code applies. Called once from `kmain` after logging is up.
pub fn report() {
    let (start, end) = range();
    let (count, bad) = relocations();
    match bad {
        None => log_info!(target: "selftest",
            "KSELFTEST: kernel image ok (base=0x{:x} len=0x{:x} relocs={})", start, end - start, count),
        Some(ty) => log_info!(target: "selftest",
            "KSELFTEST: kernel image FAIL (reloc type {} unsupported; base=0x{:x} relocs={})", ty, start, count),
    }
}
