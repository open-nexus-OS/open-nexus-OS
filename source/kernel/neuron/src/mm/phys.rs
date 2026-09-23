// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the one seam between a physical address and the kernel's view of
//! it (RFC-0098 C4, TASK-0286 P2). The kernel lives in the Sv39 high half
//! behind a direct map, `KVA = PHYS_OFFSET + PA`, over every memory bank and
//! every device window the tree names; the image itself runs there too (the
//! static PIE is fixed up twice: once at its load address, once at the high
//! alias). Before the switch the kernel runs at its load address with paging
//! off, and a physical address IS the pointer — `PAGING_ON` tells the two
//! phases apart, so the early console and the trap-time console are the same
//! code. Nothing kernel-owned lies below `KERNEL_VA_BASE`; "user address" is
//! `va < KERNEL_VA_BASE` everywhere a machine constant used to decide it.
//! The boot table that carries the switch is built here as well: one root
//! page of 1 GiB leaves — the high alias of every range the tree names plus
//! the identity of the gigabyte the switch code runs in.
//! NOT target-gated (pure arithmetic over statics) so the switch table and
//! the seam are host-tested; `mod mm` is riscv/none-only.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests` below (seam both phases, boot table for both goldens'
//!   shapes); the switch itself is proven by every QEMU boot (`KINIT: kernel
//!   high half`)
//! INVARIANTS: `PHYS_OFFSET` covers every PA below 256 GiB; `virt_to_phys` is
//!   the inverse of `phys_to_virt` in both phases; the boot table maps RAM and
//!   windows only (never a guessed range).

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Where PA 0 lives in the kernel half: the bottom of the Sv39 negative range.
pub const PHYS_OFFSET: usize = 0xffff_ffc0_0000_0000;
/// Everything at or above this VA is the kernel's.
pub const KERNEL_VA_BASE: usize = PHYS_OFFSET;
/// The largest physical address the direct map can name (256 GiB).
pub const PHYS_LIMIT: usize = 1 << 38;
/// log2 of the boot table's leaf: one gigabyte per root entry.
pub const GIB_SHIFT: u32 = 30;
/// Root entries of an Sv39 table.
pub const ROOT_ENTRIES: usize = 512;
/// V | R | W | X | G | A | D — a boot leaf: everything, accessed and dirty so
/// no A/D update can trap before the trap vector exists.
pub const BOOT_LEAF_FLAGS: usize = 0b1110_1111;
/// `satp.MODE` for Sv39.
const SATP_MODE_SV39: usize = 8 << 60;

static PAGING_ON: AtomicBool = AtomicBool::new(false);
/// The boot table's `satp`, published by the boot hart for the secondaries'
/// switch (they read it PC-relatively with paging off).
static BOOT_SATP: AtomicUsize = AtomicUsize::new(0);

/// Flip the seam to the high half. Called once, first thing after the switch.
pub fn set_paging_on(boot_satp: usize) {
    BOOT_SATP.store(boot_satp, Ordering::Release);
    PAGING_ON.store(true, Ordering::Release);
}

/// The kernel runs in the high half.
pub fn paging_on() -> bool {
    PAGING_ON.load(Ordering::Acquire)
}

/// The boot table's `satp` (0 before the switch).
pub fn boot_satp() -> usize {
    BOOT_SATP.load(Ordering::Acquire)
}

/// `pa` as the kernel may dereference it in the current phase.
#[inline]
pub fn phys_to_virt(pa: usize) -> usize {
    translate(pa, paging_on())
}

/// The physical address behind a kernel pointer, in either phase.
#[inline]
pub fn virt_to_phys(va: usize) -> usize {
    if va >= KERNEL_VA_BASE {
        va - PHYS_OFFSET
    } else {
        va
    }
}

/// The address belongs to the kernel half.
#[inline]
pub fn is_kernel_va(va: usize) -> bool {
    va >= KERNEL_VA_BASE
}

/// The pure seam: with paging on a PA is reached through the direct map,
/// before that it is the pointer.
#[inline]
pub fn translate(pa: usize, paging_on: bool) -> usize {
    debug_assert!(pa < PHYS_LIMIT, "physical address beyond the direct map");
    if paging_on {
        pa + PHYS_OFFSET
    } else {
        pa
    }
}

/// The boot table's root page.
#[repr(C, align(4096))]
pub struct BootRoot(pub [usize; ROOT_ENTRIES]);

impl BootRoot {
    pub const fn new() -> Self {
        Self([0; ROOT_ENTRIES])
    }
}

impl Default for BootRoot {
    fn default() -> Self {
        Self::new()
    }
}

/// A 1 GiB leaf for the gigabyte containing `pa`.
fn gib_leaf(pa: usize) -> usize {
    let base = (pa >> GIB_SHIFT) << GIB_SHIFT;
    ((base >> 12) << 10) | BOOT_LEAF_FLAGS
}

/// Fill `root` with the high alias of every `(base, len)` range plus the
/// identity of the gigabyte holding `identity_pa`. Ranges beyond the direct
/// map are refused (`false`); the table is left as built so far.
pub fn build_boot_table(
    root: &mut BootRoot,
    ranges: impl Iterator<Item = (usize, usize)>,
    identity_pa: usize,
) -> bool {
    for (base, len) in ranges {
        if len == 0 {
            continue;
        }
        let Some(last) = base.checked_add(len - 1) else { return false };
        if last >= PHYS_LIMIT {
            return false;
        }
        for gib in (base >> GIB_SHIFT)..=(last >> GIB_SHIFT) {
            root.0[ROOT_ENTRIES / 2 + gib] = gib_leaf(gib << GIB_SHIFT);
        }
    }
    if identity_pa >= PHYS_LIMIT {
        return false;
    }
    root.0[identity_pa >> GIB_SHIFT] = gib_leaf(identity_pa);
    true
}

/// `satp` for a root page at physical `root_pa`, ASID 0.
pub fn satp_for_root(root_pa: usize) -> usize {
    SATP_MODE_SV39 | (root_pa >> 12)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seam_is_the_pointer_before_the_switch_and_the_alias_after() {
        assert_eq!(translate(0x8040_0000, false), 0x8040_0000);
        assert_eq!(translate(0x8040_0000, true), 0xffff_ffc0_8040_0000);
        assert_eq!(translate(0, true), PHYS_OFFSET);
        assert_eq!(
            translate(0x1_0000_0000, true),
            0xffff_ffc1_0000_0000,
            "the board's second bank"
        );
        for pa in [0usize, 0x1000, 0x8040_0000, 0xd401_7000, 0x1_7fff_f000] {
            assert_eq!(virt_to_phys(translate(pa, true)), pa);
            assert_eq!(virt_to_phys(translate(pa, false)), pa);
        }
        assert!(is_kernel_va(KERNEL_VA_BASE));
        assert!(!is_kernel_va(0x7fff_ffff));
        assert!(!is_kernel_va(0x8000_0000), "the old identity range is user space now");
    }

    #[test]
    fn boot_table_for_the_board_shape() {
        // Two banks around the 2 GiB hole, a console window in gigabyte 3,
        // the kernel loaded at 6 MiB (the shape of the board golden).
        let mut root = BootRoot::new();
        let ranges =
            [(0usize, 0x8000_0000usize), (0x1_0000_0000, 0x8000_0000), (0xd401_7000, 0x100)];
        assert!(build_boot_table(&mut root, ranges.into_iter(), 0x0060_0000));
        let leaf = |gib: usize| (((gib << GIB_SHIFT) >> 12) << 10) | BOOT_LEAF_FLAGS;
        assert_eq!(root.0[256], leaf(0));
        assert_eq!(root.0[257], leaf(1));
        assert_eq!(root.0[258], 0, "the hole");
        assert_eq!(root.0[259], leaf(3), "the console's gigabyte");
        assert_eq!(root.0[260], leaf(4));
        assert_eq!(root.0[261], leaf(5));
        assert_eq!(root.0[0], leaf(0), "identity for the switch");
        assert_eq!(root.0[1..256].iter().filter(|e| **e != 0).count(), 0);
        assert_eq!(root.0[262..].iter().filter(|e| **e != 0).count(), 0);
    }

    #[test]
    fn boot_table_for_the_virt_shape_and_the_refusals() {
        // One bank at 2 GiB, two device windows in gigabyte 0, the kernel
        // loaded 4 MiB into the bank (the shape of the virt golden).
        let mut root = BootRoot::new();
        let ranges =
            [(0x8000_0000usize, 0x1400_0000usize), (0x1200_0000, 0x100), (0x0d00_0000, 0x60_0000)];
        assert!(build_boot_table(&mut root, ranges.into_iter(), 0x8040_0000));
        assert_eq!(root.0[258] & BOOT_LEAF_FLAGS, BOOT_LEAF_FLAGS);
        assert_eq!(root.0[256] & BOOT_LEAF_FLAGS, BOOT_LEAF_FLAGS, "both windows share gigabyte 0");
        assert_eq!(root.0[2] & BOOT_LEAF_FLAGS, BOOT_LEAF_FLAGS, "identity at 2 GiB");
        assert_eq!(root.0[257], 0);
        let mut root = BootRoot::new();
        assert!(!build_boot_table(&mut root, [(PHYS_LIMIT, 0x1000)].into_iter(), 0));
        assert!(!build_boot_table(&mut root, [(0usize, 0usize)].into_iter(), PHYS_LIMIT));
        assert!(
            build_boot_table(&mut root, [(0usize, 0usize)].into_iter(), 0),
            "an empty range is skipped"
        );
        assert_eq!(satp_for_root(0x8041_0000), (8 << 60) | 0x8041_0);
    }
}
