// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the rules of memory committed on first touch (RFC-0100 §2, ADR-0077) — what a page
//! fault or a kernel access to user memory may do — as pure logic, so the decisions that would
//! let a process map what it has no right to are proven on the host. The kernel's fault path and
//! its checked user range (`mm/populate.rs`) feed these functions facts from the page table and
//! the region record and execute their verdicts. Two children hold a lazy object's own state:
//! its page table (`lazy_pages.rs`: absent / a frame / a record) and the compressed store its
//! squeezed pages live in (`lazy_store.rs`, TASK-0333 6f-1, ADR-0087).
//!
//! NOT target-gated (pure index logic, like `va_space` and `vmo_lifetime`).
//!
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6b-1)
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below — the verdict table (every right, a valid PTE, an eager
//!   region's hole, no region) and what a device is told of a lazy range (the naming bound, the
//!   runs merged — 6b-2b, ADR-0078); the page table and the store carry their own
//! INVARIANTS: a page is committed only inside a region that grants the access; a lazy page is
//!   never executable.

use alloc::vec::Vec;

/// One page.
pub const PAGE: usize = 4096;

/// What a fault or a kernel range asks of a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    Execute,
}

/// The rights a region grants or a PTE carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rights {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl Rights {
    /// Whether these rights allow `access`.
    #[must_use]
    pub fn allow(self, access: Access) -> bool {
        match access {
            Access::Read => self.read,
            Access::Write => self.write,
            Access::Execute => self.execute,
        }
    }
}

/// The region an address lies in, as a fault sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionView {
    pub rights: Rights,
    /// The region maps a lazy object: its pages are committed on touch.
    pub lazy: bool,
}

/// Why a fault is not ours to fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Violation {
    /// No region covers the address.
    NoRegion,
    /// The region does not grant the access.
    NoRight(Access),
    /// An eager region has every page mapped: a fault inside it is not a missing page.
    EagerRegion,
    /// A lazy page is data, never code.
    ExecuteLazy,
}

/// What to do with a fault (or a page of a kernel range).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Commit the page if it is not committed yet, map it, fence, resume.
    Populate,
    /// The PTE already grants the access: fence and resume.
    Spurious,
    /// Not a missing page: the fault kills, the range is `EFAULT`.
    Violation(Violation),
}

/// The verdict for `access` at an address inside `region` (if any) whose PTE carries `pte`
/// (`None` when no valid leaf maps it).
#[must_use]
pub fn classify(region: Option<RegionView>, pte: Option<Rights>, access: Access) -> Verdict {
    if pte.is_some_and(|rights| rights.allow(access)) {
        return Verdict::Spurious;
    }
    let Some(region) = region else { return Verdict::Violation(Violation::NoRegion) };
    if !region.rights.allow(access) {
        return Verdict::Violation(Violation::NoRight(access));
    }
    if !region.lazy {
        return Verdict::Violation(Violation::EagerRegion);
    }
    if access == Access::Execute {
        return Verdict::Violation(Violation::ExecuteLazy);
    }
    Verdict::Populate
}

/// The object page a fault at `va` names, in a region that maps the object from byte `offset`
/// at `region_va`.
#[must_use]
pub fn page_index(region_va: usize, offset: usize, va: usize) -> usize {
    (offset + (va & !(PAGE - 1)) - region_va) / PAGE
}

/// Pages of a lazy object one `vmo_runs` may name (ADR-0078): naming commits — a device cannot
/// fault — and the commits run under the kernel lock, so one call commits a bounded number.
pub const MAX_NAMED_PAGES: usize = 64;

/// Whether `[offset, offset + len)` of a lazy object may be named to a device in one call: not
/// empty, and spanning at most [`MAX_NAMED_PAGES`] pages.
#[must_use]
pub fn nameable(offset: usize, len: usize) -> bool {
    let Some(end) = offset.checked_add(len) else { return false };
    len != 0 && end.div_ceil(PAGE) - offset / PAGE <= MAX_NAMED_PAGES
}

/// Pieces `(pa, len)` in range order merged where one ends at the next's start: what a device is
/// told of a lazy range — as few runs as its frames allow.
#[must_use]
pub fn merge_runs(pieces: impl IntoIterator<Item = (usize, usize)>) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (pa, len) in pieces {
        match runs.last_mut() {
            Some(last) if last.0.checked_add(last.1) == Some(pa) => last.1 += len,
            _ => runs.push((pa, len)),
        }
    }
    runs
}

// The page table of one lazy object: absent / a frame / a record (6b-1; the record 6f-1).
#[path = "lazy_pages.rs"]
mod pages;
pub use pages::{CommitError, LazyPages, PageState, SqueezeError, SwapStateError};

// The compressed store a lazy object's squeezed pages live in (6f-1, ADR-0087).
#[path = "lazy_store.rs"]
pub mod store;

// The swap side of a lazy object's pages: exported and swapped records (6f-2, ADR-0088).
#[path = "lazy_swap.rs"]
pub mod swap;

// The seal of a record that leaves the kernel for the pager's disk (6f-2, ADR-0088).
#[path = "lazy_seal.rs"]
pub mod seal;

#[cfg(test)]
mod tests {
    use super::*;

    const RW: Rights = Rights { read: true, write: true, execute: false };
    const RO: Rights = Rights { read: true, write: false, execute: false };
    const RX: Rights = Rights { read: true, write: false, execute: true };

    fn lazy(rights: Rights) -> Option<RegionView> {
        Some(RegionView { rights, lazy: true })
    }

    #[test]
    fn a_touch_inside_a_lazy_region_with_its_right_populates() {
        assert_eq!(classify(lazy(RW), None, Access::Read), Verdict::Populate);
        assert_eq!(classify(lazy(RW), None, Access::Write), Verdict::Populate);
        assert_eq!(classify(lazy(RO), None, Access::Read), Verdict::Populate);
    }

    /// A valid PTE that already grants the access: another hart committed the page, or this
    /// hart cached the miss — fence and resume, never a second frame.
    #[test]
    fn a_valid_pte_with_the_right_is_spurious() {
        assert_eq!(classify(lazy(RW), Some(RW), Access::Write), Verdict::Spurious);
        assert_eq!(classify(lazy(RO), Some(RO), Access::Read), Verdict::Spurious);
        // Even outside any region a PTE that grants the access has nothing to fix.
        assert_eq!(classify(None, Some(RO), Access::Read), Verdict::Spurious);
    }

    #[test]
    fn test_reject_populate_outside_a_region() {
        assert_eq!(classify(None, None, Access::Read), Verdict::Violation(Violation::NoRegion));
        assert_eq!(classify(None, None, Access::Write), Verdict::Violation(Violation::NoRegion));
    }

    #[test]
    fn test_reject_write_fault_on_a_read_only_region() {
        assert_eq!(
            classify(lazy(RO), None, Access::Write),
            Verdict::Violation(Violation::NoRight(Access::Write))
        );
        // A present read-only PTE does not let a store through either.
        assert_eq!(
            classify(lazy(RO), Some(RO), Access::Write),
            Verdict::Violation(Violation::NoRight(Access::Write))
        );
    }

    #[test]
    fn test_reject_exec_fault() {
        assert_eq!(
            classify(lazy(RW), None, Access::Execute),
            Verdict::Violation(Violation::NoRight(Access::Execute))
        );
        // Even a region that would grant execute never runs a lazy page.
        assert_eq!(
            classify(lazy(RX), None, Access::Execute),
            Verdict::Violation(Violation::ExecuteLazy)
        );
    }

    /// An eager region is fully mapped from the start: a fault inside it is a real fault.
    #[test]
    fn test_reject_populating_an_eager_region() {
        let eager = Some(RegionView { rights: RW, lazy: false });
        assert_eq!(
            classify(eager, None, Access::Write),
            Verdict::Violation(Violation::EagerRegion)
        );
    }

    #[test]
    fn a_fault_names_the_object_page_of_its_region() {
        let (region_va, offset) = (0x5000_0000, 3 * PAGE);
        assert_eq!(page_index(region_va, offset, region_va), 3);
        assert_eq!(page_index(region_va, offset, region_va + PAGE + 17), 4);
        assert_eq!(page_index(region_va, 0, region_va + 10 * PAGE + PAGE - 1), 10);
    }

    /// A device is told a lazy range as few runs as its frames allow: pages whose frames meet are
    /// one run, a gap starts the next — a partial first and last page included.
    #[test]
    fn named_pages_merge_into_runs_where_their_frames_meet() {
        let pieces = [
            (0x8000_0f00, PAGE - 0xf00),
            (0x8000_1000, PAGE),
            (0x9000_0000, PAGE),
            (0x9000_1000, 0x10),
        ];
        assert_eq!(merge_runs(pieces), [(0x8000_0f00, PAGE + 0x100), (0x9000_0000, PAGE + 0x10)]);
        assert!(merge_runs([]).is_empty());
        assert_eq!(merge_runs([(0x8000_0000, PAGE)]), [(0x8000_0000, PAGE)]);
    }

    /// Naming commits under the kernel lock: one call names at most `MAX_NAMED_PAGES` pages,
    /// counted as the pages the range spans, and never an empty or overflowing range.
    #[test]
    fn test_reject_naming_more_pages_at_once_than_the_bound() {
        assert!(nameable(0, MAX_NAMED_PAGES * PAGE));
        assert!(nameable(5 * PAGE + 17, 100), "an inner byte range spans one page");
        assert!(!nameable(0, MAX_NAMED_PAGES * PAGE + 1));
        assert!(!nameable(PAGE - 1, (MAX_NAMED_PAGES - 1) * PAGE + 2), "65 pages spanned");
        assert!(!nameable(PAGE, 0));
        assert!(!nameable(usize::MAX - 1, 2));
    }
}
