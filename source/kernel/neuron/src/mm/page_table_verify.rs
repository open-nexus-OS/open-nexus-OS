// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Debug-only Sv39 invariant checker for [`super::page_table::PageTable`] —
//! split out of the module (structure-gate, RFC-0085 Phase 2). Compiled only
//! under `debug_assertions`; the release kernel carries none of it.

#[cfg(debug_assertions)]
use super::page_table::{PageFlags, PageTable, PageTablePage, LEAF_PERMS, PT_ENTRIES};

#[cfg(debug_assertions)]
impl PageTable {
    /// Debug-only invariant checker for the Sv39 page table.
    /// Verifies that:
    /// - Non-leaf entries do not carry leaf permission bits
    /// - Leaf entries are VALID and carry at least one of R/W/X
    /// - W^X is enforced (never both WRITE and EXECUTE)
    ///   This is a best-effort walk that assumes the internal pointers
    ///   are well-formed; only compiled when debug assertions or the
    ///   `debug_pt_verify` feature is enabled.
    #[cfg(debug_assertions)]
    pub fn verify(&self) -> Result<(), &'static str> {
        unsafe fn walk(page: *const PageTablePage) -> Result<(), &'static str> {
            for i in 0..PT_ENTRIES {
                let entry = unsafe { (*page).entries[i] };
                if entry == 0 {
                    continue;
                }
                let valid = entry & PageFlags::VALID.bits() != 0;
                if !valid {
                    return Err("pt: nonzero but !VALID");
                }
                let is_leaf = entry & LEAF_PERMS.bits() != 0;
                if is_leaf {
                    let has_perm = entry & LEAF_PERMS.bits() != 0;
                    if !has_perm {
                        return Err("pt: leaf without perms");
                    }
                    let w = entry & PageFlags::WRITE.bits() != 0;
                    let x = entry & PageFlags::EXECUTE.bits() != 0;
                    if w && x {
                        return Err("pt: W^X violated");
                    }
                } else {
                    // Non-leaf must not carry any leaf perms
                    if entry & LEAF_PERMS.bits() != 0 {
                        return Err("pt: non-leaf has leaf perms");
                    }
                    let next = ((entry >> 10) << 12) as *const PageTablePage;
                    // Recurse into the next level
                    unsafe { walk(next)? };
                }
            }
            Ok(())
        }

        unsafe { walk(self.root.as_ptr()) }
    }
}
